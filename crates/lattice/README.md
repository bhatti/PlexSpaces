# Lattice - CRDT Implementations

**Purpose**: Provides Conflict-Free Replicated Data Type (CRDT) lattice implementations that enable coordination-free distributed systems.

## Overview

This module has **NO dependencies** on other PlexSpaces modules. It only uses standard Rust types and serde for serialization.

## Lattice Types

- **LWWLattice**: Last-Writer-Wins for simple values
- **SetLattice**: Add-only or add-remove set (OR-Set)
- **MaxLattice**: Maximum value lattice
- **MinLattice**: Minimum value lattice
- **MapLattice**: Map with lattice values
- **VectorClock**: Causal ordering

## Usage Examples

### LWWLattice

```rust
use plexspaces_lattice::LWWLattice;

let mut lattice = LWWLattice::new(42);
lattice.update(100, timestamp1);
lattice.update(200, timestamp2);  // Wins if timestamp2 > timestamp1
```

### SetLattice

```rust
use plexspaces_lattice::SetLattice;

let mut set = SetLattice::new();
set.add("item1".to_string());
set.add("item2".to_string());
set.remove("item1".to_string());
```

## Dependencies

This crate has **NO dependencies** on other PlexSpaces modules.

## Dependents

This crate is used by:
- `plexspaces_tuplespace`: Lattice-based TupleSpace implementation

## References

- Implementation: `crates/lattice/src/`
- Tests: `crates/lattice/tests/`
- Inspiration: CRDT research, Riak, AntidoteDB

