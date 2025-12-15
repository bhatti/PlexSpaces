// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Simple LRU cache implementation with fixed size and TTL expiration

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, SystemTime};

/// LRU cache with fixed size and TTL expiration
///
/// ## Design
/// - Fixed capacity (evicts least recently used when full)
/// - TTL expiration (entries older than TTL are considered expired)
/// - O(1) average-case lookup and insertion
///
/// ## Usage
/// ```rust,ignore
/// let mut cache = LruCache::new(1000, Duration::from_secs(3600));
/// cache.insert("key1", value1);
/// if let Some(value) = cache.get("key1") {
///     // Use value
/// }
/// ```
pub struct LruCache<K, V> {
    /// Maximum number of entries (fixed size)
    capacity: usize,
    /// TTL for entries (entries older than this are expired)
    ttl: Duration,
    /// Map for O(1) lookups (key -> (value, timestamp))
    map: HashMap<K, (V, SystemTime)>,
    /// Queue for LRU ordering (most recent at back, least recent at front)
    /// Used to track access order for eviction
    queue: VecDeque<K>,
}

impl<K, V> LruCache<K, V>
where
    K: Hash + Eq + Clone,
{
    /// Create a new LRU cache with fixed capacity and TTL
    ///
    /// ## Arguments
    /// * `capacity` - Maximum number of entries (fixed size)
    /// * `ttl` - Time-to-live for entries (entries older than this are expired)
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            map: HashMap::with_capacity(capacity),
            queue: VecDeque::with_capacity(capacity),
        }
    }

    /// Get value for key (returns None if not found or expired)
    ///
    /// Updates LRU order (moves key to back of queue)
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let now = SystemTime::now();
        
        // Check if key exists
        if !self.map.contains_key(key) {
            return None;
        }
        
        // Check if expired (need to check timestamp first)
        let expired = self.map.get(key)
            .map(|(_, timestamp)| {
                now.duration_since(*timestamp).unwrap_or_default() >= self.ttl
            })
            .unwrap_or(true);
        
        if expired {
            // Expired - remove it
            self.remove(key);
            return None;
        }
        
        // Update LRU order: move to back of queue
        // Find and remove from queue (O(n) but acceptable for cache sizes)
        if let Some(pos) = self.queue.iter().position(|k| k == key) {
            self.queue.remove(pos);
        }
        // Add to back (most recently used)
        self.queue.push_back(key.clone());
        
        // Return reference to value
        self.map.get(key).map(|(value, _)| value)
    }

    /// Insert key-value pair
    ///
    /// If cache is full, evicts least recently used entry.
    /// If key already exists, updates value and LRU order.
    pub fn insert(&mut self, key: K, value: V) {
        let now = SystemTime::now();
        
        // Check if key already exists
        if let Some((old_value, timestamp)) = self.map.get_mut(&key) {
            // Update existing entry
            *old_value = value;
            *timestamp = now;
            
            // Update LRU order: move to back
            if let Some(pos) = self.queue.iter().position(|k| k == &key) {
                self.queue.remove(pos);
            }
            self.queue.push_back(key.clone());
            return;
        }
        
        // New entry - check if we need to evict
        if self.map.len() >= self.capacity {
            // Evict least recently used (front of queue)
            if let Some(lru_key) = self.queue.pop_front() {
                self.map.remove(&lru_key);
            }
        }
        
        // Insert new entry
        self.queue.push_back(key.clone());
        self.map.insert(key, (value, now));
    }

    /// Remove key from cache
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some((value, _)) = self.map.remove(key) {
            // Remove from queue
            if let Some(pos) = self.queue.iter().position(|k| k == key) {
                self.queue.remove(pos);
            }
            Some(value)
        } else {
            None
        }
    }

    /// Cleanup expired entries
    ///
    /// Removes all entries older than TTL
    pub fn cleanup_expired(&mut self) {
        let now = SystemTime::now();
        let expired_keys: Vec<K> = self
            .map
            .iter()
            .filter(|(_, (_, timestamp))| {
                now.duration_since(*timestamp).unwrap_or_default() >= self.ttl
            })
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in expired_keys {
            self.remove(&key);
        }
    }

    /// Get current size (number of entries)
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clear all entries
    pub fn clear(&mut self) {
        self.map.clear();
        self.queue.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lru_cache_basic() {
        let mut cache = LruCache::new(3, Duration::from_secs(3600));
        
        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        cache.insert("key3", "value3");
        
        assert_eq!(cache.get(&"key1"), Some(&"value1"));
        assert_eq!(cache.get(&"key2"), Some(&"value2"));
        assert_eq!(cache.get(&"key3"), Some(&"value3"));
    }

    #[test]
    fn test_lru_cache_eviction() {
        let mut cache = LruCache::new(2, Duration::from_secs(3600));
        
        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        cache.insert("key3", "value3"); // Should evict key1 (least recently used)
        
        assert_eq!(cache.get(&"key1"), None); // Evicted
        assert_eq!(cache.get(&"key2"), Some(&"value2"));
        assert_eq!(cache.get(&"key3"), Some(&"value3"));
    }

    #[test]
    fn test_lru_cache_ttl_expiration() {
        let mut cache = LruCache::new(10, Duration::from_millis(100));
        
        cache.insert("key1", "value1");
        assert_eq!(cache.get(&"key1"), Some(&"value1"));
        
        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        
        // Should be expired
        assert_eq!(cache.get(&"key1"), None);
    }

    #[test]
    fn test_lru_cache_update_existing() {
        let mut cache = LruCache::new(3, Duration::from_secs(3600));
        
        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        cache.insert("key1", "value1_updated"); // Update existing
        
        assert_eq!(cache.get(&"key1"), Some(&"value1_updated"));
        assert_eq!(cache.len(), 2); // Still only 2 entries
    }

    #[test]
    fn test_lru_cache_cleanup_expired() {
        let mut cache = LruCache::new(10, Duration::from_millis(100));
        
        cache.insert("key1", "value1");
        cache.insert("key2", "value2");
        
        // Wait for expiration
        std::thread::sleep(Duration::from_millis(150));
        
        // Cleanup expired
        cache.cleanup_expired();
        
        assert_eq!(cache.len(), 0);
    }
}
