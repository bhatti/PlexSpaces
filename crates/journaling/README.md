# Journaling - Durable Execution and Event Sourcing

**Purpose**: Provides durable execution and event sourcing for PlexSpaces actors, enabling exactly-once semantics, deterministic replay, and time-travel debugging through RESTATE-inspired journaling.

## Overview

This crate implements **Pillar 3: Durability** (Restate-inspired) of the PlexSpaces architecture. It is **100% optional** via the DurabilityFacet pattern (Static vs Dynamic design principle).

### Component Diagram

```text
┌─────────────────────────────────────────────────────────────────┐
│ Actor (plexspaces-actor)                                        │
│   └─ DurabilityFacet ──┐                                        │
└─────────────────────────│────────────────────────────────────────┘
                          │
                          v
┌─────────────────────────────────────────────────────────────────┐
│ JournalStorage Trait (this crate)                              │
│   ├─ MemoryJournalStorage   (testing)                          │
│   ├─ PostgresJournalStorage (production)                       │
│   ├─ RedisJournalStorage    (distributed)                      │
│   └─ SqliteJournalStorage   (edge)                             │
└─────────────────────────────────────────────────────────────────┘
                          │
                          v
┌─────────────────────────────────────────────────────────────────┐
│ Journal Table (PostgreSQL/SQLite) or Redis                     │
│   ├─ journal_entries   (append-only)                           │
│   └─ checkpoints       (periodic snapshots)                    │
└─────────────────────────────────────────────────────────────────┘
```

## Key Components

### JournalStorage Trait

Pluggable journal backends:

```rust
#[async_trait]
pub trait JournalStorage: Send + Sync {
    async fn append_entry(&self, entry: &JournalEntry) -> Result<(), JournalError>;
    async fn replay_from(&self, actor_id: &str, from_sequence: u64) -> Result<Vec<JournalEntry>, JournalError>;
    async fn get_latest_checkpoint(&self, actor_id: &str) -> Result<Option<Checkpoint>, JournalError>;
    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> Result<(), JournalError>;
}
```

### ExecutionContext

Tracks replay mode and caches side effects:

```rust
pub struct ExecutionContext {
    pub is_replay: bool,
    pub side_effects: Vec<SideEffect>,
    pub cached_results: HashMap<String, Vec<u8>>,
}
```

### CheckpointManager

Periodic snapshots for fast recovery (90%+ faster):

```rust
pub struct CheckpointManager {
    storage: Arc<dyn JournalStorage>,
    checkpoint_interval: u64,
    compression: CompressionType,
}
```

### DurabilityFacet

Optional actor capability for durable execution:

```rust
pub struct DurabilityFacet {
    storage: Arc<dyn JournalStorage>,
    config: DurabilityConfig,
    execution_context: ExecutionContext,
}
```

## Backend Implementations

### MemoryJournalStorage

In-memory storage for testing:

```rust
let storage = MemoryJournalStorage::new();
```

### SqliteJournalStorage

SQLite backend for edge deployments:

```rust
let storage = SqliteJournalStorage::new(":memory:").await?;
```

### PostgresJournalStorage

PostgreSQL backend for production:

```rust
let storage = PostgresJournalStorage::new("postgresql://localhost/plexspaces").await?;
```

### RedisJournalStorage

Redis backend for distributed deployments:

```rust
let storage = RedisJournalStorage::new("redis://localhost:6379").await?;
```

## Usage Examples

### Basic Usage (Memory Backend)

```rust
use plexspaces_journaling::*;
use plexspaces_proto::prost_types;

// Create in-memory journal (for testing)
let storage = MemoryJournalStorage::new();

// Create journal entry with MessageReceived
let entry = JournalEntry {
    id: ulid::Ulid::new().to_string(),
    actor_id: "actor-123".to_string(),
    sequence: 1,
    timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
    correlation_id: "corr-1".to_string(),
    entry: Some(journal_entry::Entry::MessageReceived(MessageReceived {
        message_id: "msg-1".to_string(),
        sender_id: "sender-123".to_string(),
        message_type: "test".to_string(),
        payload: vec![1, 2, 3],
        metadata: Default::default(),
    })),
};
storage.append_entry(&entry).await?;

// Replay from sequence
let entries = storage.replay_from("actor-123", 0).await?;
for entry in entries {
    // Process entry for deterministic replay
}
```

### Production Usage (SQLite Backend)

```rust
use plexspaces_journaling::*;

// Create SQLite journal for production (persistent storage)
let storage = SqliteJournalStorage::new("/var/lib/plexspaces/journal.db").await?;

// Configure durability with checkpointing
let config = DurabilityConfig {
    backend: JournalBackend::JournalBackendSqlite as i32,
    checkpoint_interval: 1000,  // Checkpoint every 1000 messages
    checkpoint_timeout: None,
    replay_on_activation: true,
    cache_side_effects: true,
    compression: CompressionType::CompressionTypeZstd as i32,  // Enable compression
    backend_config: None,
};

let facet = DurabilityFacet::new(storage, config);
// Attach to actor for durable execution
```

### Attaching Durability to Actor

```rust
use plexspaces_journaling::*;
use plexspaces_facet::Facet;

// Create storage backend
let storage = SqliteJournalStorage::new(":memory:").await?;

// Configure durability
let config = DurabilityConfig {
    backend: JournalBackend::JournalBackendSqlite as i32,
    checkpoint_interval: 100,  // Checkpoint every 100 messages
    checkpoint_timeout: None,
    replay_on_activation: true,  // Replay journal on restart
    cache_side_effects: true,
    compression: CompressionType::CompressionTypeNone as i32,
    backend_config: None,
};

let durability_facet = DurabilityFacet::new(Arc::new(storage), config);

// Attach to actor
actor.facets().attach(Box::new(durability_facet))?;
```

## Design Principles

### Exactly-Once Semantics

All messages and state changes are journaled, enabling deterministic replay and exactly-once processing.

### Deterministic Replay

Actors can be replayed from any point in their history, enabling:
- Crash recovery
- Time-travel debugging
- State reconstruction

### Checkpointing

Periodic snapshots (checkpoints) enable fast recovery:
- Checkpoint every N messages (configurable)
- Compressed checkpoints (zstd) for space efficiency
- 90%+ faster recovery vs full replay

### Side Effect Caching

During replay, side effects (network calls, file I/O) are cached to ensure deterministic behavior.

## Performance Characteristics

- **Journal append**: < 1ms (in-memory), < 5ms (SQLite), < 10ms (PostgreSQL)
- **Replay speed**: ~1000 entries/second (full replay)
- **Checkpoint recovery**: ~10000 entries/second (from checkpoint)
- **Checkpoint size**: ~10% of journal size (with compression)

## Testing

```bash
# Run all journaling tests
cargo test -p plexspaces-journaling

# Run with specific backend
cargo test -p plexspaces-journaling --features sqlite-backend
cargo test -p plexspaces-journaling --features postgres-backend
cargo test -p plexspaces-journaling --features redis-backend

# Check coverage
cargo tarpaulin -p plexspaces-journaling --out Html
```

## Dependencies

This crate depends on:
- `plexspaces_core`: Common types (ActorId, ActorContext, errors)
- `plexspaces_proto`: Protocol buffer definitions for journal entry types
- `plexspaces_mailbox`: Message types for journaling
- `sqlx`: PostgreSQL and SQLite backends (optional, feature-gated)
- `redis`: Redis backend (optional, feature-gated)
- `zstd`: Checkpoint compression (optional, feature-gated)

## Dependents

This crate is used by:
- `plexspaces_actor`: Via DurabilityFacet for optional durability
- `plexspaces_supervisor`: For crash recovery and replay

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - Journaling](../../docs/detailed-design.md#journaling) - Comprehensive journaling documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with durable actors
- Implementation: `crates/journaling/src/`
- Tests: `crates/journaling/src/` (unit tests)
- Proto definitions: `proto/plexspaces/v1/journaling.proto`
- Inspiration: [Restate](https://restate.dev/) durable execution model

