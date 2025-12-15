# Persistence - Event Sourcing and State Persistence

**Purpose**: Provides journaling and snapshot capabilities for durable actor execution.

## Overview

This crate provides event sourcing and state persistence for actors, enabling:
- **Event Sourcing**: Append-only event log
- **Snapshots**: Periodic state snapshots for fast recovery
- **Replay**: Deterministic replay from events

## Key Components

### ExecutionContext

Tracks replay mode and caches side effects:

```rust
pub struct ExecutionContext {
    pub is_replay: bool,
    pub side_effects: Vec<SideEffect>,
    pub cached_results: HashMap<String, Vec<u8>>,
}
```

### Codec

Compression and encryption for persistence:

```rust
pub mod codec {
    pub fn compress(data: &[u8]) -> Result<Vec<u8>>;
    pub fn decompress(data: &[u8]) -> Result<Vec<u8>>;
    pub fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>>;
    pub fn decrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>>;
}
```

## Dependencies

This crate depends on:
- `plexspaces_core`: Common types
- `plexspaces_proto`: Protocol buffer definitions

## Dependents

This crate is used by:
- `plexspaces_journaling`: Journaling uses persistence for storage

## References

- Implementation: `crates/persistence/src/`
- Tests: `crates/persistence/tests/`

