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

//! Lattice-based TupleSpace for coordination-free distributed coordination
//!
//! This implementation uses lattices to enable coordination-free TupleSpace
//! operations, allowing multiple nodes to operate independently and merge
//! their states without locks or consensus.

use crate::{Pattern, Tuple};
use plexspaces_lattice::{ConsistencyLevel, Lattice, OrSetLattice, VectorClock};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use ulid::Ulid;

/// A lattice-based TupleSpace that supports coordination-free operations
#[derive(Clone)]
pub struct LatticeTupleSpace {
    /// The tuples stored as an OR-Set lattice for add/remove support
    tuples: Arc<RwLock<OrSetLattice<Tuple>>>,

    /// Vector clock for causal consistency
    clock: Arc<RwLock<VectorClock>>,

    /// Node ID for this replica
    node_id: String,

    /// Consistency level for operations
    #[allow(dead_code)]
    consistency: ConsistencyLevel,
}

impl LatticeTupleSpace {
    /// Create a new lattice-based TupleSpace
    pub fn new(node_id: String, consistency: ConsistencyLevel) -> Self {
        LatticeTupleSpace {
            tuples: Arc::new(RwLock::new(OrSetLattice::new())),
            clock: Arc::new(RwLock::new(VectorClock::new())),
            node_id,
            consistency,
        }
    }

    /// Write a tuple (coordination-free)
    pub async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        let mut tuples = self.tuples.write().await;
        let mut clock = self.clock.write().await;

        // Generate unique ID for this write
        let unique_id = format!("{}-{}", self.node_id, Ulid::new());

        // Add tuple with unique ID
        tuples.add(tuple, unique_id);

        // Update vector clock
        clock.inc(self.node_id.clone());

        Ok(())
    }

    /// Read tuples matching a pattern (local, no coordination)
    pub async fn read(&self, pattern: &Pattern) -> Vec<Tuple> {
        let tuples = self.tuples.read().await;

        tuples
            .elements()
            .filter(|tuple| pattern.matches(tuple))
            .cloned()
            .collect()
    }

    /// Take a tuple (remove it)
    /// Note: In a distributed setting, this may conflict with concurrent takes
    pub async fn take(&self, pattern: &Pattern) -> Option<Tuple> {
        let mut tuples = self.tuples.write().await;
        let mut clock = self.clock.write().await;

        // Find first matching tuple
        let matching = tuples
            .elements()
            .find(|tuple| pattern.matches(tuple))
            .cloned();

        if let Some(ref tuple) = matching {
            // Remove the tuple
            tuples.remove(tuple);

            // Update vector clock
            clock.inc(self.node_id.clone());
        }

        matching
    }

    /// Merge with another TupleSpace replica (anti-entropy)
    pub async fn merge(&self, other: &LatticeTupleSpace) -> Result<(), TupleSpaceError> {
        let mut our_tuples = self.tuples.write().await;
        let their_tuples = other.tuples.read().await;

        let mut our_clock = self.clock.write().await;
        let their_clock = other.clock.read().await;

        // Merge tuples using lattice merge
        *our_tuples = our_tuples.merge(&*their_tuples);

        // Merge vector clocks
        *our_clock = our_clock.merge(&*their_clock);

        Ok(())
    }

    /// Get the current vector clock (for causal consistency)
    pub async fn get_clock(&self) -> VectorClock {
        self.clock.read().await.clone()
    }

    /// Check if an operation is causally consistent
    pub async fn is_causal(&self, other_clock: &VectorClock) -> bool {
        let our_clock = self.clock.read().await;
        !our_clock.concurrent(other_clock)
    }

    /// Gossip with a peer (bidirectional merge)
    pub async fn gossip(&self, peer: &LatticeTupleSpace) -> Result<(), TupleSpaceError> {
        // Get both states
        let our_tuples = self.tuples.read().await.clone();
        let their_tuples = peer.tuples.read().await.clone();

        // Compute merged state
        let merged_tuples = our_tuples.merge(&their_tuples);

        // Update both replicas
        *self.tuples.write().await = merged_tuples.clone();
        *peer.tuples.write().await = merged_tuples;

        // Merge clocks
        let our_clock = self.clock.read().await.clone();
        let their_clock = peer.clock.read().await.clone();
        let merged_clock = our_clock.merge(&their_clock);

        *self.clock.write().await = merged_clock.clone();
        *peer.clock.write().await = merged_clock;

        Ok(())
    }

    /// Get a snapshot for persistence or transmission
    pub async fn snapshot(&self) -> TupleSpaceSnapshot {
        TupleSpaceSnapshot {
            tuples: self.tuples.read().await.clone(),
            clock: self.clock.read().await.clone(),
            node_id: self.node_id.clone(),
        }
    }

    /// Restore from a snapshot
    pub async fn restore(&self, snapshot: TupleSpaceSnapshot) -> Result<(), TupleSpaceError> {
        *self.tuples.write().await = snapshot.tuples;
        *self.clock.write().await = snapshot.clock;
        Ok(())
    }
}

/// Snapshot of TupleSpace state
#[derive(Clone, Serialize, Deserialize)]
pub struct TupleSpaceSnapshot {
    /// Tuples stored in OR-Set CRDT for conflict-free replication
    pub tuples: OrSetLattice<Tuple>,
    /// Vector clock for causality tracking
    pub clock: VectorClock,
    /// Node identifier that created this snapshot
    pub node_id: String,
}

/// TupleSpace errors
#[derive(Debug, thiserror::Error)]
pub enum TupleSpaceError {
    /// Consistency level requirement violated
    #[error("Consistency violation: {0}")]
    ConsistencyViolation(String),

    /// Conflict during CRDT merge operation
    #[error("Merge conflict: {0}")]
    MergeConflict(String),

    /// Network communication error with peer nodes
    #[error("Network error: {0}")]
    NetworkError(String),
}

/// Anti-entropy gossip protocol for TupleSpace replicas
pub struct TupleSpaceGossiper {
    local: Arc<LatticeTupleSpace>,
    peers: Vec<Arc<LatticeTupleSpace>>,
    interval: std::time::Duration,
}

impl TupleSpaceGossiper {
    /// Creates a new gossiper for anti-entropy synchronization
    ///
    /// # Arguments
    /// * `local` - Local TupleSpace replica to synchronize
    /// * `peers` - Remote TupleSpace replicas to gossip with
    /// * `interval` - Time interval between gossip rounds
    pub fn new(
        local: Arc<LatticeTupleSpace>,
        peers: Vec<Arc<LatticeTupleSpace>>,
        interval: std::time::Duration,
    ) -> Self {
        TupleSpaceGossiper {
            local,
            peers,
            interval,
        }
    }

    /// Run the gossip protocol
    pub async fn run(&self) {
        loop {
            tokio::time::sleep(self.interval).await;

            // Pick random peer
            if let Some(peer) = self.peers.get(rand::random::<usize>() % self.peers.len()) {
                // Gossip with peer
                if let Err(e) = self.local.gossip(peer).await {
                    eprintln!("Gossip error: {:?}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PatternField, TupleField};

    #[tokio::test]
    async fn test_lattice_tuplespace_write_read() {
        let space = LatticeTupleSpace::new("node1".to_string(), ConsistencyLevel::Eventual);

        // Write tuples
        let tuple1 = Tuple::new(vec!["user".into(), "alice".into(), 25.into()]);
        let tuple2 = Tuple::new(vec!["user".into(), "bob".into(), 30.into()]);

        space.write(tuple1.clone()).await.unwrap();
        space.write(tuple2.clone()).await.unwrap();

        // Read with pattern
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("user".to_string())),
            PatternField::Wildcard, // Wildcard
            PatternField::Wildcard, // Wildcard
        ]);

        let results = space.read(&pattern).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_lattice_tuplespace_merge() {
        let space1 = LatticeTupleSpace::new("node1".to_string(), ConsistencyLevel::Eventual);
        let space2 = LatticeTupleSpace::new("node2".to_string(), ConsistencyLevel::Eventual);

        // Write different tuples to each space
        let tuple1 = Tuple::new(vec!["data".into(), "from_node1".into()]);
        let tuple2 = Tuple::new(vec!["data".into(), "from_node2".into()]);

        space1.write(tuple1.clone()).await.unwrap();
        space2.write(tuple2.clone()).await.unwrap();

        // Merge spaces
        space1.merge(&space2).await.unwrap();

        // Both tuples should be in space1 now
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("data".to_string())),
            PatternField::Wildcard,
        ]);
        let results = space1.read(&pattern).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_vector_clock_causality() {
        let space1 = LatticeTupleSpace::new("node1".to_string(), ConsistencyLevel::Causal);
        let space2 = LatticeTupleSpace::new("node2".to_string(), ConsistencyLevel::Causal);

        // Write to space1
        space1.write(Tuple::new(vec!["test".into()])).await.unwrap();
        let clock1 = space1.get_clock().await;

        // Write to space2
        space2
            .write(Tuple::new(vec!["test2".into()]))
            .await
            .unwrap();
        let clock2 = space2.get_clock().await;

        // Clocks should be concurrent (no causal relationship)
        assert!(clock1.concurrent(&clock2));

        // After merge, causality is established
        space1.merge(&space2).await.unwrap();
        let merged_clock = space1.get_clock().await;

        assert!(!merged_clock.concurrent(&clock1));
        assert!(!merged_clock.concurrent(&clock2));
    }
}
