// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Routes messages to partitions by hashing the target actor ID.
//!
//! Uses FNV-1a for speed (no cryptographic requirement).
//! Same target_actor_id always maps to the same partition,
//! preserving per-recipient message ordering.

/// Routes a target actor ID to a partition index.
pub struct HashRouter {
    num_partitions: u32,
}

impl HashRouter {
    /// Create a router for the given number of partitions.
    pub fn new(num_partitions: u32) -> Self {
        assert!(num_partitions > 0, "num_partitions must be > 0");
        Self { num_partitions }
    }

    /// Map a target actor ID to a partition index in [0, num_partitions).
    pub fn route(&self, target_actor_id: &str) -> u32 {
        let hash = fnv1a(target_actor_id.as_bytes());
        hash % self.num_partitions
    }

    /// Number of partitions this router targets.
    pub fn num_partitions(&self) -> u32 {
        self.num_partitions
    }
}

fn fnv1a(data: &[u8]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &b in data {
        hash ^= b as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deterministic_routing() {
        let router = HashRouter::new(8);
        let p1 = router.route("actor-123");
        let p2 = router.route("actor-123");
        assert_eq!(p1, p2, "same ID must always map to same partition");
    }

    #[test]
    fn test_partition_range() {
        let router = HashRouter::new(16);
        for i in 0..1000 {
            let p = router.route(&format!("actor-{}", i));
            assert!(p < 16, "partition {} out of range", p);
        }
    }

    #[test]
    fn test_distribution() {
        let n = 8u32;
        let router = HashRouter::new(n);
        let mut counts = vec![0u32; n as usize];
        for i in 0..10_000 {
            let p = router.route(&format!("target-actor-{}", i));
            counts[p as usize] += 1;
        }
        for (i, &c) in counts.iter().enumerate() {
            assert!(c > 500, "partition {} got only {} — severely unbalanced", i, c);
        }
    }

    #[test]
    fn test_single_partition() {
        let router = HashRouter::new(1);
        assert_eq!(router.route("anything"), 0);
        assert_eq!(router.route("something-else"), 0);
    }

    #[test]
    #[should_panic(expected = "num_partitions must be > 0")]
    fn test_zero_partitions_panics() {
        HashRouter::new(0);
    }

    #[test]
    fn test_different_ids_spread() {
        let router = HashRouter::new(4);
        let mut seen = std::collections::HashSet::new();
        for i in 0..100 {
            seen.insert(router.route(&format!("actor-{}", i)));
        }
        assert!(seen.len() > 1, "all 100 IDs mapped to same partition");
    }
}
