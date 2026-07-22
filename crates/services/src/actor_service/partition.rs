// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Partition strategies for ShardGroup routing
//!
//! ## Purpose
//! Implements partitioning strategies for routing messages to shards in a ShardGroup.
//! Inspired by NSDI'22 Data-Parallel Actors paper.
//!
//! ## Strategies
//! - **Hash**: Simple hash-based partitioning (uniform distribution)
//! - **ConsistentHash**: Consistent hashing with virtual nodes (minimal rebalancing)
//! - **Range**: Range-based partitioning (ordered keys)

use plexspaces_proto::actor::v1::PartitionStrategy;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// Calculate shard ID from partition key using specified strategy
///
/// ## Arguments
/// * `partition_key` - Key to partition on (bytes)
/// * `strategy` - Partition strategy enum value
/// * `shard_count` - Total number of shards
/// * `range_ranges` - Optional range boundaries for Range partitioning (sorted ascending)
///
/// ## Returns
/// Shard ID (0 to shard_count-1)
pub fn calculate_shard_id(
    partition_key: &[u8],
    strategy: i32,
    shard_count: u32,
    range_ranges: Option<&[Vec<u8>]>, // Range boundaries for Range partitioning
) -> Result<u32, String> {
    if shard_count == 0 {
        return Err("shard_count must be > 0".to_string());
    }

    match strategy {
        x if x == PartitionStrategy::PartitionStrategyHash as i32 => {
            hash_partition(partition_key, shard_count)
        }
        x if x == PartitionStrategy::PartitionStrategyConsistentHash as i32 => {
            consistent_hash_partition(partition_key, shard_count)
        }
        x if x == PartitionStrategy::PartitionStrategyRange as i32 => {
            range_partition(partition_key, shard_count, range_ranges)
        }
        _ => Err(format!("Unsupported partition strategy: {}", strategy)),
    }
}

/// Hash-based partitioning (uniform distribution)
///
/// ## Algorithm
/// ```
/// hash(key) % shard_count
/// ```
///
/// ## Pros
/// - Uniform distribution
/// - Simple implementation
///
/// ## Cons
/// - Full reshuffle on scale (4→8 shards = ~50% keys move)
///
/// ## Use Cases
/// - Uniform access patterns
/// - Infrequent scaling
fn hash_partition(partition_key: &[u8], shard_count: u32) -> Result<u32, String> {
    let mut hasher = DefaultHasher::new();
    partition_key.hash(&mut hasher);
    let hash = hasher.finish();
    Ok((hash % shard_count as u64) as u32)
}

/// Consistent hashing with virtual nodes (minimal rebalancing)
///
/// ## Algorithm
/// ```
/// 1. Create virtual nodes: Each shard has V virtual nodes (default V=100)
/// 2. Hash each virtual node: hash("shard-{i}-vn-{j}") → position on ring
/// 3. Hash partition key: hash(key) → position on ring
/// 4. Find closest virtual node clockwise → shard
/// ```
///
/// ## Pros
/// - Minimal key movement on scale (1/N keys move)
/// - Better load distribution with virtual nodes
///
/// ## Cons
/// - More complex than hash
/// - Requires virtual node management
///
/// ## Use Cases
/// - Frequent scaling
/// - Large datasets
/// - Need minimal rebalancing
fn consistent_hash_partition(partition_key: &[u8], shard_count: u32) -> Result<u32, String> {
    const VIRTUAL_NODES_PER_SHARD: u32 = 100; // Default virtual nodes per shard

    // Hash the partition key to get position on ring
    let mut hasher = DefaultHasher::new();
    partition_key.hash(&mut hasher);
    let key_hash = hasher.finish();

    // Find closest virtual node clockwise
    let mut best_shard = 0u32;
    let mut best_distance = u64::MAX;

    // Check all virtual nodes for all shards
    for shard_id in 0..shard_count {
        for vn in 0..VIRTUAL_NODES_PER_SHARD {
            // Hash virtual node identifier: "shard-{shard_id}-vn-{vn}"
            let vn_key = format!("shard-{}-vn-{}", shard_id, vn);
            let mut vn_hasher = DefaultHasher::new();
            vn_key.hash(&mut vn_hasher);
            let vn_hash = vn_hasher.finish();

            // Calculate clockwise distance on ring
            let distance = if vn_hash >= key_hash {
                vn_hash - key_hash
            } else {
                // Wrap around: distance = (max - key_hash) + vn_hash
                (u64::MAX - key_hash) + vn_hash + 1
            };

            if distance < best_distance {
                best_distance = distance;
                best_shard = shard_id;
            }
        }
    }

    Ok(best_shard)
}

/// Range-based partitioning (ordered keys)
///
/// ## Algorithm
/// ```
/// 1. Define ranges: [range_0, range_1, ..., range_N-1]
/// 2. Compare key with ranges (binary search)
/// 3. Return shard for matching range
/// ```
///
/// ## Pros
/// - Efficient range queries (query single shard)
/// - Preserves key ordering
///
/// ## Cons
/// - Requires range boundaries
/// - Potential hotspots if ranges uneven
///
/// ## Use Cases
/// - Time-series data (timestamp ranges)
/// - Ordered keys (e.g., user IDs in ranges)
/// - Range queries common
fn range_partition(
    partition_key: &[u8],
    shard_count: u32,
    range_ranges: Option<&[Vec<u8>]>,
) -> Result<u32, String> {
    // If no ranges provided, use simple byte comparison
    // Shard i handles keys where: key >= range[i] && key < range[i+1]
    if let Some(ranges) = range_ranges {
        if ranges.len() != shard_count as usize {
            return Err(format!(
                "Range boundaries count ({}) must match shard_count ({})",
                ranges.len(),
                shard_count
            ));
        }

        // Binary search for matching range
        for (i, range_boundary) in ranges.iter().enumerate() {
            if partition_key < range_boundary.as_slice() {
                return Ok(i as u32);
            }
        }

        // Key >= last boundary, assign to last shard
        Ok(shard_count - 1)
    } else {
        // No ranges provided: use simple byte-based range partitioning
        // Divide key space evenly: shard = (first_byte * shard_count) / 256
        if partition_key.is_empty() {
            return Err("Partition key cannot be empty for range partitioning".to_string());
        }

        let first_byte = partition_key[0] as u32;
        let shard_id = (first_byte * shard_count) / 256;
        Ok(shard_id.min(shard_count - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_partition() {
        let key1 = b"user-001";
        let key2 = b"user-002";
        let shard_count = 4;

        let shard1 = hash_partition(key1, shard_count).unwrap();
        let shard2 = hash_partition(key2, shard_count).unwrap();

        assert!(shard1 < shard_count);
        assert!(shard2 < shard_count);

        // Same key should map to same shard
        let shard1_again = hash_partition(key1, shard_count).unwrap();
        assert_eq!(shard1, shard1_again);
    }

    #[test]
    fn test_consistent_hash_partition() {
        let key1 = b"user-001";
        let key2 = b"user-002";
        let shard_count = 4;

        let shard1 = consistent_hash_partition(key1, shard_count).unwrap();
        let shard2 = consistent_hash_partition(key2, shard_count).unwrap();

        assert!(shard1 < shard_count);
        assert!(shard2 < shard_count);

        // Same key should map to same shard
        let shard1_again = consistent_hash_partition(key1, shard_count).unwrap();
        assert_eq!(shard1, shard1_again);
    }

    #[test]
    fn test_range_partition() {
        let shard_count = 4;

        // Test with explicit ranges
        let ranges = vec![
            b"a".to_vec(), // Shard 0: < "a"
            b"m".to_vec(), // Shard 1: "a" <= key < "m"
            b"t".to_vec(), // Shard 2: "m" <= key < "t"
            b"z".to_vec(), // Shard 3: "t" <= key < "z"
        ];

        assert_eq!(
            range_partition(b"apple", shard_count, Some(&ranges)).unwrap(),
            1
        );
        assert_eq!(
            range_partition(b"zebra", shard_count, Some(&ranges)).unwrap(),
            3
        );
        assert_eq!(
            range_partition(b"0", shard_count, Some(&ranges)).unwrap(),
            0
        );

        // Test without ranges (byte-based)
        let shard = range_partition(b"\x40", shard_count, None).unwrap();
        assert!(shard < shard_count);
    }
}
