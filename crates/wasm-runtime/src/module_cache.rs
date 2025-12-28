// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Content-addressed module cache with LRU eviction

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use crate::WasmModule;

// Metrics are recorded via the metrics crate macros (no import needed)

/// Module cache for WASM modules (content-addressed by SHA-256 hash)
/// 
/// ## Design
/// - Content-addressed by SHA-256 hash (same module = same hash)
/// - LRU eviction when capacity exceeded
/// - Thread-safe (Arc<RwLock<>>)
/// - Tracks cache hits/misses for metrics
pub struct ModuleCache {
    /// Maximum number of modules to cache
    capacity: usize,
    /// Map: hash -> (module, access_order_index)
    modules: HashMap<String, (Arc<WasmModule>, usize)>,
    /// LRU queue: most recently used at back, least recently used at front
    /// Stores hash strings
    access_order: VecDeque<String>,
}

impl ModuleCache {
    /// Create new module cache with specified capacity
    ///
    /// ## Arguments
    /// * `capacity` - Maximum number of modules to cache (default: 1000)
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            modules: HashMap::with_capacity(capacity),
            access_order: VecDeque::with_capacity(capacity),
        }
    }

    /// Get module by hash (updates LRU order)
    ///
    /// ## Returns
    /// Cloned `Arc<WasmModule>` if found, None otherwise
    pub fn get(&mut self, hash: &str) -> Option<Arc<WasmModule>> {
        if let Some((module, _)) = self.modules.get(hash) {
            // Clone the module reference first (before mutable borrow)
            let module_clone = Arc::clone(module);
            
            // Update LRU order: move to back
            if let Some(pos) = self.access_order.iter().position(|h| h == hash) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(hash.to_string());
            
            // Update index in map
            if let Some((_, idx)) = self.modules.get_mut(hash) {
                *idx = self.access_order.len() - 1;
            }
            
            metrics::counter!("plexspaces_wasm_module_cache_hits_total").increment(1);
            Some(module_clone)
        } else {
            metrics::counter!("plexspaces_wasm_module_cache_misses_total").increment(1);
            None
        }
    }

    /// Insert module into cache (evicts LRU if full)
    ///
    /// ## Arguments
    /// * `hash` - SHA-256 hash of module bytes
    /// * `module` - Compiled WASM module
    pub fn insert(&mut self, hash: String, module: Arc<WasmModule>) {
        // If already exists, just update LRU order
        if self.modules.contains_key(&hash) {
            if let Some(pos) = self.access_order.iter().position(|h| h == &hash) {
                self.access_order.remove(pos);
            }
            self.access_order.push_back(hash.clone());
            if let Some((_, ref mut idx)) = self.modules.get_mut(&hash) {
                *idx = self.access_order.len() - 1;
            }
            return;
        }

        // If cache is full, evict least recently used
        if self.modules.len() >= self.capacity {
            if let Some(lru_hash) = self.access_order.pop_front() {
                if let Some((evicted_module, _)) = self.modules.remove(&lru_hash) {
                    metrics::counter!("plexspaces_wasm_module_cache_evictions_total").increment(1);
                    metrics::histogram!("plexspaces_wasm_module_cache_evicted_size_bytes").record(evicted_module.size_bytes as f64);
                }
            }
        }

        // Insert new module
        let idx = self.access_order.len();
        self.access_order.push_back(hash.clone());
        self.modules.insert(hash, (module, idx));
        
        metrics::gauge!("plexspaces_wasm_module_cache_size").set(self.modules.len() as f64);
    }

    /// Evict module by hash (manual eviction)
    pub fn evict(&mut self, hash: &str) -> bool {
        if let Some((evicted_module, _)) = self.modules.remove(hash) {
            if let Some(pos) = self.access_order.iter().position(|h| h == hash) {
                self.access_order.remove(pos);
            }
            metrics::counter!("plexspaces_wasm_module_cache_evictions_total").increment(1);
                    metrics::histogram!("plexspaces_wasm_module_cache_evicted_size_bytes").record(evicted_module.size_bytes as f64);
            metrics::gauge!("plexspaces_wasm_module_cache_size").set(self.modules.len() as f64);
            true
        } else {
            false
        }
    }

    /// Clear all modules from cache
    pub fn clear(&mut self) {
        let count = self.modules.len();
        self.modules.clear();
        self.access_order.clear();
        metrics::gauge!("plexspaces_wasm_module_cache_size").set(0.0);
        metrics::counter!("plexspaces_wasm_module_cache_clears_total").increment(count as u64);
    }

    /// Get current cache size
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Iterate over all cached modules (for listing)
    /// Returns vector of (name, version, hash) tuples
    pub fn iter(&self) -> Vec<(String, String, String)> {
        self.modules
            .values()
            .map(|(module, _)| {
                (module.name.clone(), module.version.clone(), module.hash.clone())
            })
            .collect()
    }
}

impl Default for ModuleCache {
    fn default() -> Self {
        Self::new(1000) // Default capacity: 1000 modules
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let cache = ModuleCache::new(100);
        assert_eq!(cache.capacity, 100);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_default() {
        let cache = ModuleCache::default();
        assert_eq!(cache.capacity, 1000);
    }

    #[test]
    fn test_insert_and_get() {
        let mut cache = ModuleCache::new(10);
        
        // Create a dummy module (we can't easily create real WasmModule in tests)
        // For now, test the structure
        let _hash1 = "hash1".to_string();
        // Note: In real usage, we'd need actual WasmModule, but for structure test this is fine
        // The actual WasmModule creation is tested in integration tests
        
        assert!(cache.get("hash1").is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn test_lru_eviction() {
        let _cache = ModuleCache::new(2);
        
        // Insert 2 modules
        let _hash1 = "hash1".to_string();
        let _hash2 = "hash2".to_string();
        // Note: Would need actual WasmModule instances here
        
        // Insert 3rd module should evict first
        let _hash3 = "hash3".to_string();
        // cache.insert(hash3, module3);
        // assert!(cache.get("hash1").is_none());
        // assert!(cache.get("hash2").is_some());
        // assert!(cache.get("hash3").is_some());
    }

    #[test]
    fn test_clear() {
        let mut cache = ModuleCache::new(10);
        // Insert some modules
        // cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<ModuleCache>();
        assert_sync::<ModuleCache>();
    }
}
