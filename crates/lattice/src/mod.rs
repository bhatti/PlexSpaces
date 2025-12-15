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

//! Lattice-based CRDT implementation for coordination-free distributed state
//!
//! Based on the CALM theorem and Anna KVS design, this module provides
//! lattice data structures with Associative, Commutative, and Idempotent (ACI)
//! merge operations for conflict-free distributed state management.

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

/// Core lattice trait with ACI merge properties
pub trait Lattice: Clone + Send + Sync + 'static + PartialEq {
    /// Merge with another lattice value
    /// Must be associative, commutative, and idempotent
    fn merge(&self, other: &Self) -> Self;

    /// Check if this value subsumes (dominates) another
    fn subsumes(&self, other: &Self) -> bool {
        self.merge(other) == *self
    }

    /// Get the bottom element (identity for merge)
    fn bottom() -> Self;
}

/// Last-Writer-Wins lattice
/// Resolves conflicts by keeping the value with the highest timestamp
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LWWLattice<T: Clone> {
    /// The actual value stored
    pub value: T,
    /// Timestamp when this value was written (for conflict resolution)
    pub timestamp: u64,
    /// Writer identifier for deterministic tie-breaking
    pub writer_id: String,
}

impl<T: Clone + Send + Sync + 'static> LWWLattice<T> {
    /// Creates a new Last-Writer-Wins lattice value
    ///
    /// # Arguments
    /// * `value` - The value to store
    /// * `timestamp` - Timestamp when this value was written
    /// * `writer_id` - Unique identifier of the writer (for tie-breaking)
    pub fn new(value: T, timestamp: u64, writer_id: String) -> Self {
        LWWLattice {
            value,
            timestamp,
            writer_id,
        }
    }
}

impl<T: Clone + Send + Sync + 'static + PartialEq> Lattice for LWWLattice<T> {
    fn merge(&self, other: &Self) -> Self {
        match self.timestamp.cmp(&other.timestamp) {
            Ordering::Less => other.clone(),
            Ordering::Greater => self.clone(),
            Ordering::Equal => {
                // Tie-break with writer_id for determinism
                if other.writer_id > self.writer_id {
                    other.clone()
                } else {
                    self.clone()
                }
            }
        }
    }

    fn bottom() -> Self {
        panic!("LWWLattice requires initial value")
    }
}

/// Set lattice - grows monotonically by union
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetLattice<T: Ord + Clone> {
    /// Elements in the set
    pub elements: BTreeSet<T>,
}

impl<T: Ord + Clone + Send + Sync + 'static> Default for SetLattice<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone + Send + Sync + 'static> SetLattice<T> {
    /// Creates a new empty set lattice
    pub fn new() -> Self {
        SetLattice {
            elements: BTreeSet::new(),
        }
    }

    /// Creates a set lattice with a single element
    pub fn singleton(value: T) -> Self {
        let mut elements = BTreeSet::new();
        elements.insert(value);
        SetLattice { elements }
    }

    /// Checks if the set contains the given value
    pub fn contains(&self, value: &T) -> bool {
        self.elements.contains(value)
    }

    /// Returns the number of elements in the set
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns true if the set contains no elements
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

impl<T: Ord + Clone + Send + Sync + 'static> Lattice for SetLattice<T> {
    fn merge(&self, other: &Self) -> Self {
        SetLattice {
            elements: self.elements.union(&other.elements).cloned().collect(),
        }
    }

    fn bottom() -> Self {
        SetLattice::new()
    }
}

/// Counter lattice - vector clock style
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CounterLattice {
    /// Per-actor counters for distributed counting
    pub counts: HashMap<String, u64>,
}

impl Default for CounterLattice {
    fn default() -> Self {
        Self::new()
    }
}

impl CounterLattice {
    /// Creates a new empty counter lattice
    pub fn new() -> Self {
        CounterLattice {
            counts: HashMap::new(),
        }
    }

    /// Creates a counter lattice with a single actor's count
    pub fn inc(actor_id: String, amount: u64) -> Self {
        let mut counts = HashMap::new();
        counts.insert(actor_id, amount);
        CounterLattice { counts }
    }

    /// Returns the total count across all actors
    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Gets the count for a specific actor
    pub fn get(&self, actor_id: &str) -> u64 {
        self.counts.get(actor_id).copied().unwrap_or(0)
    }
}

impl Lattice for CounterLattice {
    fn merge(&self, other: &Self) -> Self {
        let mut merged = self.counts.clone();
        for (id, &count) in &other.counts {
            merged
                .entry(id.clone())
                .and_modify(|c| *c = (*c).max(count))
                .or_insert(count);
        }
        CounterLattice { counts: merged }
    }

    fn bottom() -> Self {
        CounterLattice::new()
    }
}

/// Max lattice - keeps maximum value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaxLattice<T: Ord + Clone> {
    /// The maximum value
    pub value: T,
}

impl<T: Ord + Clone + Send + Sync + 'static> MaxLattice<T> {
    /// Creates a new max lattice with the given value
    pub fn new(value: T) -> Self {
        MaxLattice { value }
    }
}

impl<T: Ord + Clone + Send + Sync + 'static> Lattice for MaxLattice<T>
where
    T: Default,
{
    fn merge(&self, other: &Self) -> Self {
        MaxLattice {
            value: self.value.clone().max(other.value.clone()),
        }
    }

    fn bottom() -> Self {
        MaxLattice {
            value: T::default(),
        }
    }
}

/// Min lattice - keeps minimum value
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MinLattice<T: Ord + Clone> {
    /// The minimum value
    pub value: T,
}

impl<T: Ord + Clone + Send + Sync + 'static> MinLattice<T> {
    /// Creates a new min lattice with the given value
    pub fn new(value: T) -> Self {
        MinLattice { value }
    }
}

impl<T: Ord + Clone + Send + Sync + 'static> Lattice for MinLattice<T>
where
    T: Default,
{
    fn merge(&self, other: &Self) -> Self {
        MinLattice {
            value: self.value.clone().min(other.value.clone()),
        }
    }

    fn bottom() -> Self {
        MinLattice {
            value: T::default(),
        }
    }
}

/// Vector clock for causal consistency
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorClock {
    /// Per-node logical clocks for causality tracking
    pub clocks: HashMap<String, u64>,
}

impl Default for VectorClock {
    fn default() -> Self {
        Self::new()
    }
}

impl VectorClock {
    /// Creates a new empty vector clock
    pub fn new() -> Self {
        VectorClock {
            clocks: HashMap::new(),
        }
    }

    /// Increments the clock for the given node
    pub fn inc(&mut self, node_id: String) {
        self.clocks
            .entry(node_id)
            .and_modify(|c| *c += 1)
            .or_insert(1);
    }

    /// Gets the clock value for a specific node
    pub fn get(&self, node_id: &str) -> u64 {
        self.clocks.get(node_id).copied().unwrap_or(0)
    }

    /// Check if this clock happens-before another
    pub fn happens_before(&self, other: &Self) -> bool {
        self.clocks.iter().all(|(k, &v)| other.get(k) >= v)
            && self.clocks.len() <= other.clocks.len()
    }

    /// Check if two clocks are concurrent (neither happens-before the other)
    pub fn concurrent(&self, other: &Self) -> bool {
        !self.happens_before(other) && !other.happens_before(self)
    }
}

impl Lattice for VectorClock {
    fn merge(&self, other: &Self) -> Self {
        let mut merged = self.clocks.clone();
        for (id, &clock) in &other.clocks {
            merged
                .entry(id.clone())
                .and_modify(|c| *c = (*c).max(clock))
                .or_insert(clock);
        }
        VectorClock { clocks: merged }
    }

    fn bottom() -> Self {
        VectorClock::new()
    }
}

/// Pair lattice - combines two lattices
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairLattice<A: Lattice, B: Lattice> {
    /// First lattice component
    pub first: A,
    /// Second lattice component
    pub second: B,
}

impl<A: Lattice, B: Lattice> PairLattice<A, B> {
    /// Creates a new pair lattice from two lattices
    pub fn new(first: A, second: B) -> Self {
        PairLattice { first, second }
    }
}

impl<A: Lattice, B: Lattice> Lattice for PairLattice<A, B> {
    fn merge(&self, other: &Self) -> Self {
        PairLattice {
            first: self.first.merge(&other.first),
            second: self.second.merge(&other.second),
        }
    }

    fn bottom() -> Self {
        PairLattice {
            first: A::bottom(),
            second: B::bottom(),
        }
    }
}

/// Map lattice - maps keys to lattice values
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapLattice<K: Ord + Clone + std::hash::Hash + Eq, V: Lattice> {
    /// Entries mapping keys to lattice values
    pub entries: HashMap<K, V>,
}

impl<K: Ord + Clone + Send + Sync + 'static, V: Lattice> Default for MapLattice<K, V>
where
    K: std::hash::Hash + Eq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone + Send + Sync + 'static, V: Lattice> MapLattice<K, V>
where
    K: std::hash::Hash + Eq,
{
    /// Creates a new empty map lattice
    pub fn new() -> Self {
        MapLattice {
            entries: HashMap::new(),
        }
    }

    /// Inserts or merges a value at the given key
    pub fn insert(&mut self, key: K, value: V) {
        self.entries
            .entry(key)
            .and_modify(|v| *v = v.merge(&value))
            .or_insert(value);
    }

    /// Gets the lattice value for a specific key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.get(key)
    }
}

impl<K, V> Lattice for MapLattice<K, V>
where
    K: Ord + Clone + Send + Sync + 'static + std::hash::Hash + Eq,
    V: Lattice,
{
    fn merge(&self, other: &Self) -> Self {
        let mut merged = self.entries.clone();
        for (k, v) in &other.entries {
            merged
                .entry(k.clone())
                .and_modify(|existing| *existing = existing.merge(v))
                .or_insert(v.clone());
        }
        MapLattice { entries: merged }
    }

    fn bottom() -> Self {
        MapLattice::new()
    }
}

/// Observed-Remove Set for removable sets
///
/// A CRDT that allows both additions and removals with conflict-free semantics.
/// Uses unique IDs to track additions so concurrent add/remove operations are handled correctly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OrSetLattice<T: Ord + Clone + std::hash::Hash + Eq> {
    added: HashMap<T, BTreeSet<String>>, // value -> unique IDs
    removed: BTreeSet<String>,           // removed IDs
}

impl<T: Ord + Clone + Send + Sync + 'static + std::hash::Hash + Eq> Default for OrSetLattice<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Ord + Clone + Send + Sync + 'static + std::hash::Hash + Eq> OrSetLattice<T> {
    /// Creates a new empty OR-Set
    pub fn new() -> Self {
        OrSetLattice {
            added: HashMap::new(),
            removed: BTreeSet::new(),
        }
    }

    /// Adds a value with a unique ID for tracking
    pub fn add(&mut self, value: T, unique_id: String) {
        self.added.entry(value).or_default().insert(unique_id);
    }

    /// Removes a value by marking all its IDs as removed
    pub fn remove(&mut self, value: &T) {
        if let Some(ids) = self.added.get(value) {
            for id in ids {
                self.removed.insert(id.clone());
            }
        }
    }

    /// Checks if the set contains the value (not marked as removed)
    pub fn contains(&self, value: &T) -> bool {
        if let Some(ids) = self.added.get(value) {
            ids.iter().any(|id| !self.removed.contains(id))
        } else {
            false
        }
    }

    /// Returns an iterator over all non-removed elements
    pub fn elements(&self) -> impl Iterator<Item = &T> {
        self.added
            .iter()
            .filter(|(_, ids)| ids.iter().any(|id| !self.removed.contains(id)))
            .map(|(v, _)| v)
    }
}

impl<T> Lattice for OrSetLattice<T>
where
    T: Ord + Clone + Send + Sync + 'static + std::hash::Hash + Eq,
{
    fn merge(&self, other: &Self) -> Self {
        let mut merged_added = self.added.clone();
        for (v, ids) in &other.added {
            merged_added
                .entry(v.clone())
                .or_default()
                .extend(ids.clone());
        }

        OrSetLattice {
            added: merged_added,
            removed: self.removed.union(&other.removed).cloned().collect(),
        }
    }

    fn bottom() -> Self {
        OrSetLattice::new()
    }
}

/// Consistency level for operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyLevel {
    /// Basic lattice merge, no ordering guarantees
    Eventual,
    /// Causal consistency with vector clocks
    Causal,
    /// Read committed with transaction IDs
    ReadCommitted,
    /// Read uncommitted (fastest, may see partial updates)
    ReadUncommitted,
    /// Linearizable (requires coordination, slowest)
    Linearizable,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lww_lattice() {
        let lww1 = LWWLattice::new("value1", 100, "node1".to_string());
        let lww2 = LWWLattice::new("value2", 200, "node2".to_string());

        let merged = lww1.merge(&lww2);
        assert_eq!(merged.value, "value2");
        assert_eq!(merged.timestamp, 200);

        // Test idempotence
        let merged2 = merged.merge(&merged);
        assert_eq!(merged, merged2);
    }

    #[test]
    fn test_set_lattice() {
        let mut set1 = SetLattice::singleton("a");
        let mut set2 = SetLattice::singleton("b");

        let merged = set1.merge(&set2);
        assert!(merged.contains(&"a"));
        assert!(merged.contains(&"b"));
        assert_eq!(merged.len(), 2);

        // Test commutativity
        let merged2 = set2.merge(&set1);
        assert_eq!(merged, merged2);
    }

    #[test]
    fn test_counter_lattice() {
        let counter1 = CounterLattice::inc("node1".to_string(), 5);
        let counter2 = CounterLattice::inc("node2".to_string(), 3);

        let merged = counter1.merge(&counter2);
        assert_eq!(merged.total(), 8);
        assert_eq!(merged.get("node1"), 5);
        assert_eq!(merged.get("node2"), 3);
    }

    #[test]
    fn test_vector_clock() {
        let mut vc1 = VectorClock::new();
        vc1.inc("node1".to_string());

        let mut vc2 = VectorClock::new();
        vc2.inc("node2".to_string());

        assert!(vc1.concurrent(&vc2));

        let merged = vc1.merge(&vc2);
        assert!(merged.happens_before(&merged)); // Reflexive
        assert_eq!(merged.get("node1"), 1);
        assert_eq!(merged.get("node2"), 1);
    }

    #[test]
    fn test_or_set() {
        let mut orset = OrSetLattice::new();
        orset.add("item1", "id1".to_string());
        orset.add("item1", "id2".to_string());
        orset.add("item2", "id3".to_string());

        assert!(orset.contains(&"item1"));
        assert!(orset.contains(&"item2"));

        orset.remove(&"item1");
        assert!(!orset.contains(&"item1"));
        assert!(orset.contains(&"item2"));
    }
}
