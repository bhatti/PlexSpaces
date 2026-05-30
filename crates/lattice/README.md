# plexspaces-lattice

**Purpose**: Conflict-Free Replicated Data Type (CRDT) lattice implementations for coordination-free distributed state management.

## Overview

All types implement the [`Lattice`] trait with Associative, Commutative, and Idempotent (ACI) merge semantics. This crate has **no dependencies** on other PlexSpaces crates.

## Lattice Types

| Type | Description |
|------|-------------|
| `LWWLattice<T>` | Last-Writer-Wins — keeps value with highest timestamp |
| `SetLattice<T>` | Add-only monotonic set |
| `OrSetLattice<T>` | Observed-Remove set — supports conflict-free add and remove |
| `CounterLattice` | Distributed vector-clock counter |
| `MaxLattice<T>` | Keeps maximum value |
| `MinLattice<T>` | Keeps minimum value |
| `MapLattice<K,V>` | Map with lattice values, merges on key conflict |
| `VectorClock` | Causal ordering via logical clocks |
| `PairLattice<A,B>` | Combines two independent lattices |

## Usage Examples

### LWWLattice

```rust
use plexspaces_lattice::{LWWLattice, Lattice};

let a = LWWLattice::new("hello", 100, "node1".to_string());
let b = LWWLattice::new("world", 200, "node2".to_string());
let merged = a.merge(&b);
assert_eq!(merged.value, "world"); // higher timestamp wins
```

### SetLattice

```rust
use plexspaces_lattice::{SetLattice, Lattice};

let s1 = SetLattice::singleton("item1");
let s2 = SetLattice::singleton("item2");
let merged = s1.merge(&s2);
assert!(merged.contains(&"item1"));
assert!(merged.contains(&"item2"));
```

### CounterLattice

```rust
use plexspaces_lattice::{CounterLattice, Lattice};

let c1 = CounterLattice::inc("node1".to_string(), 5);
let c2 = CounterLattice::inc("node2".to_string(), 3);
let merged = c1.merge(&c2);
assert_eq!(merged.total(), 8);
```

### VectorClock

```rust
use plexspaces_lattice::VectorClock;

let mut vc1 = VectorClock::new();
vc1.inc("node1".to_string());
let mut vc2 = VectorClock::new();
vc2.inc("node2".to_string());
assert!(vc1.concurrent(&vc2));
```

## Dependents

- `plexspaces-tuplespace`: Lattice-based TupleSpace implementation

## References

- Source: `crates/lattice/src/crdt.rs`
- Inspiration: CRDT research, Anna KVS, CALM theorem
