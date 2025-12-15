# Durable Actor Example

This example demonstrates all features of durable execution in PlexSpaces:

## Features Demonstrated

1. **Journaling**: All actor operations (messages, side effects) are automatically journaled
2. **Checkpoints**: Periodic state snapshots for fast recovery (90%+ faster than full replay)
3. **Deterministic Replay**: Actor state is recovered from journal on restart
4. **Side Effect Caching**: External calls are cached during replay to prevent duplicates
5. **Exactly-Once Semantics**: Guarantees no duplicate side effects

## How It Works

The example simulates a counter actor that:
- Processes increment/decrement messages
- Makes external API calls (side effects)
- Can be restarted and recover state from journal

### Execution Flow

1. **Normal Operation**:
   - Actor processes messages
   - Side effects are executed and journaled
   - Checkpoints are created periodically

2. **Recovery**:
   - Actor restarts (simulated crash)
   - Latest checkpoint is loaded
   - Journal entries after checkpoint are replayed
   - Side effects are loaded from cache (not re-executed)

## Quick Start

```bash
# Run the example directly
cargo run --release --bin durable_actor_example --features sqlite-backend

# Or use the run script
./scripts/run.sh

# Run tests and validation
./scripts/test.sh

# Run integration tests only
cargo test --features sqlite-backend -- --nocapture
```

## Architecture

```
┌─────────────────────────────────────────┐
│ Actor (Counter)                         │
│   └─ DurabilityFacet ──┐                │
└─────────────────────────│────────────────┘
                          │
                          v
┌─────────────────────────────────────────┐
│ JournalStorage (SQLite)                 │
│   ├─ journal_entries (append-only)      │
│   └─ checkpoints (periodic snapshots)   │
└─────────────────────────────────────────┘
```

## Key Components

- **DurabilityFacet**: Optional capability that adds durability to actors
- **JournalStorage**: Pluggable backend (SQLite, PostgreSQL, Redis, Memory)
- **ExecutionContext**: Tracks replay mode and caches side effects
- **CheckpointManager**: Creates periodic snapshots for fast recovery

## Configuration

The example uses:
- **checkpoint_interval**: 10 (checkpoint every 10 journal entries)
- **replay_on_activation**: true (replay journal on actor restart)
- **cache_side_effects**: true (cache external calls during replay)

## Benefits

1. **Fault Tolerance**: Actors can recover from crashes
2. **Exactly-Once Execution**: No duplicate side effects
3. **Fast Recovery**: 90%+ faster with checkpoints
4. **Time-Travel Debugging**: Can replay any point in actor history
5. **Transparent**: No changes to actor code required

