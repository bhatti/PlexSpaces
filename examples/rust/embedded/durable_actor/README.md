# Durable Actor Example

**Purpose**: Demonstrate actor durability with journaling and replay.

**Pattern**: All operations journaled, state recovered on restart.

## Quick Start

```bash
cd examples/rust/embedded/durable_actor

# Build
cargo build

# Run
cargo run

# Run with debug logging
RUST_LOG=debug cargo run

# Run in release mode
cargo run --release
```

## What It Demonstrates

1. **Journaling**: All operations persisted before execution
2. **Checkpoints**: Periodic state snapshots for fast recovery
3. **Deterministic Replay**: State recovered from journal on restart
4. **Side Effect Caching**: External calls cached (exactly-once semantics)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ Actor with DurabilityFacet                                      │
│   ├─ before_method() → journal operation                        │
│   ├─ execute()       → process message                          │
│   └─ after_method()  → journal result                           │
└─────────────────────────────────────────────────────────────────┘
                              │
                              v
┌─────────────────────────────────────────────────────────────────┐
│ JournalStorage                                                  │
│   ├─ journal_entries (append-only)                              │
│   └─ checkpoints (periodic snapshots)                           │
└─────────────────────────────────────────────────────────────────┘
```

## Key Code Patterns

### Creating Journal Storage

```rust
use plexspaces_journaling::{MemoryJournalStorage, JournalStorage};

// In-memory for testing
let storage: Arc<dyn JournalStorage> = Arc::new(MemoryJournalStorage::new());

// For production, use SQLite or PostgreSQL:
// let storage = SqliteJournalStorage::new("journal.db").await?;
```

### Configuring Durability

```rust
use plexspaces_journaling::{DurabilityConfig, JournalBackend, CompressionType};

let config = DurabilityConfig {
    backend: JournalBackend::JournalBackendMemory as i32,
    checkpoint_interval: 5,      // Checkpoint every 5 operations
    replay_on_activation: true,  // Replay journal on restart
    cache_side_effects: true,    // Cache external call results
    compression: CompressionType::CompressionTypeNone as i32,
    state_schema_version: 1,
    ..Default::default()
};
```

### Attaching DurabilityFacet

```rust
use plexspaces_journaling::DurabilityFacet;
use plexspaces_facet::Facet;

let config_json = serde_json::json!({
    "backend": config.backend,
    "checkpoint_interval": config.checkpoint_interval,
    // ... other config
});

let mut facet = DurabilityFacet::new(storage.clone(), config_json, 50);
facet.on_attach(actor_id, serde_json::json!({})).await?;
```

### Journaling Operations

```rust
// Before processing: journal the operation
facet.before_method("increment", &payload).await?;

// Execute the operation
counter += value;

// After processing: journal the result
facet.after_method("increment", &payload, &result).await?;
```

### Crash Recovery (Replay)

```rust
// Simulate crash
facet.on_detach(actor_id).await?;

// Restart with new facet (journal replayed automatically)
let mut new_facet = DurabilityFacet::new(storage.clone(), config_json, 50);
new_facet.on_attach(actor_id, serde_json::json!({})).await?;
// State is now recovered from journal
```

## Expected Output

```
Step 1: Setting up journal storage
  Storage: In-memory (use SQLite for production)

Step 2: Configuring durability
  Checkpoint interval: 5 operations
  Replay on activation: true
  Cache side effects: true

Step 3: Creating durable actor
  Actor ID: counter-actor@durable-node
  Durability facet attached

Step 4: Processing messages (journaled)
  [1/5] Increment(10) → counter = 10
  [2/5] Increment(5) → counter = 15
  [3/5] Decrement(3) → counter = 12
  [4/5] Increment(8) → counter = 20
  [5/5] GetValue → 20

Step 5: Journal statistics
  Journal entries: 10
  Latest checkpoint: sequence 10

Step 6: Simulating crash and restart
  Actor crashed (detached)
  Actor restarted (reattached)
  Journal replayed automatically

Step 7: Processing after restart
  Increment(2) → counter = 22
  Total journal entries: 12
```

## Backends

| Backend | Use Case |
|---------|----------|
| **Memory** | Testing only (not durable) |
| **SQLite** | Edge deployments, single-node |
| **PostgreSQL** | Production, multi-node |
| **Redis** | Distributed (eventually consistent) |

## Use Cases

**Use Durable Actors when:**
- Must survive crashes (financial transactions, order processing)
- Need exactly-once semantics (no duplicate operations)
- Require audit trail (all operations logged)
- Time-travel debugging needed

**Don't use when:**
- Performance is critical (journaling adds latency)
- State is easily reconstructible
- Eventual consistency is acceptable

## See Also

- [Supervision Tree](../supervision_tree/) - For fault tolerance without durability
- [Reminders](../reminders/) - For durable timers
- [Architecture Docs](../../../../docs/architecture.md)
