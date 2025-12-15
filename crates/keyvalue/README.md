# KeyValue Store - Multi-Purpose Key-Value Storage

**Purpose**: Provides a multi-purpose key-value storage abstraction for the PlexSpaces framework, supporting persistent state management, configuration storage, service discovery, distributed locks, and more.

## Overview

The KeyValue store is foundational infrastructure used throughout PlexSpaces:

- **ActorRegistry**: Maps ActorId → ActorRef for service discovery
- **NodeRegistry**: Maps NodeId → NodeInfo for cluster membership
- **Configuration**: Framework settings with hot reload via watch
- **Feature Flags**: Runtime toggles for capabilities
- **Leases**: Distributed locks with TTL
- **Workflows**: Long-running process state
- **Snapshots**: Actor state persistence
- **Metrics**: Counters and observability data
- **Sessions**: Stateful interactions with automatic expiry

## Why KeyValue NOT TupleSpace?

TupleSpace is designed for coordination (dataflow patterns) with destructive `take()` operations. Registry and configuration need **non-destructive reads** with safe lookup semantics. Industry standard (Consul, etcd, Redis) uses key-value for persistent state.

## Key Components

- **KeyValueStore**: Main trait defining all operations
- **InMemoryKVStore**: HashMap-based implementation for testing
- **SqlKVStore**: SQLite/PostgreSQL implementation for persistence
- **RedisKVStore**: Redis implementation for distributed storage

## Access Patterns

### ActorRegistry Access Patterns

**Primary Operations** (Hot Path):
- `lookup(actor_id)` - **READ HEAVY** (95% of operations)
- `register(actor_id, actor_ref)` - **WRITE** (4% of operations)
- `list_all()` - **SCAN** (1% of operations)

**Key Pattern**: `actor:{actor_id}` (e.g., `actor:payment-processor`)

**Performance Requirements**:
- Lookup latency: < 1ms (99th percentile)
- Register latency: < 5ms
- List all: < 50ms for 10K actors

### NodeRegistry Access Patterns

**Primary Operations**:
- `lookup(node_id)` - **READ HEAVY** (90% of operations)
- `register(node_id, node_info)` - **WRITE** (5% of operations)
- `list_all()` - **SCAN** (5% of operations)
- `refresh_ttl(node_id)` - **UPDATE** (every 10s per node)

**Key Pattern**: `node:{node_id}` (e.g., `node:node-1`)

**Performance Requirements**:
- Lookup latency: < 1ms
- TTL refresh: < 2ms (high frequency)
- List all: < 100ms for 1K nodes

## Performance Optimizations

### Optimized Schema

```sql
CREATE TABLE kv_store (
    key TEXT PRIMARY KEY,              -- Clustered index (fast point lookups)
    value BLOB NOT NULL,
    expires_at BIGINT,                 -- Changed to BIGINT for PostgreSQL compatibility
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

-- Index 1: TTL cleanup (composite for efficiency)
CREATE INDEX idx_kv_store_ttl_cleanup
ON kv_store(expires_at, key)
WHERE expires_at IS NOT NULL;          -- Partial index (PostgreSQL/SQLite 3.8+)

-- Index 2: Stats queries (covering index)
CREATE INDEX idx_kv_store_stats
ON kv_store(key, LENGTH(value))        -- Covering index for stats()
WHERE expires_at IS NULL OR expires_at > CURRENT_TIMESTAMP;

-- Index 3: Prefix queries (PostgreSQL)
CREATE INDEX idx_kv_store_prefix_text
ON kv_store(key text_pattern_ops);     -- PostgreSQL only

-- Index 4: Prefix queries (SQLite)
CREATE INDEX idx_kv_store_actor_prefix
ON kv_store(SUBSTR(key, 1, 6))         -- For 'actor:' prefix
WHERE key LIKE 'actor:%';

CREATE INDEX idx_kv_store_node_prefix
ON kv_store(SUBSTR(key, 1, 5))         -- For 'node:' prefix
WHERE key LIKE 'node:%';

CREATE INDEX idx_kv_store_config_prefix
ON kv_store(SUBSTR(key, 1, 7))         -- For 'config:' prefix
WHERE key LIKE 'config:%';
```

### Performance Targets

| Operation | Current | Target | Improvement |
|-----------|---------|--------|-------------|
| **Point Lookup** | 1-2ms | < 1ms | 2x faster |
| **Prefix Query (100 keys)** | 50ms | < 10ms | 5x faster |
| **TTL Cleanup (1K expired)** | 100ms | < 5ms | 20x faster |
| **Stats (100K keys)** | 200ms | < 50ms | 4x faster |
| **Batch Insert (1K keys)** | 500ms | < 100ms | 5x faster |

### SQLite Optimizations

```sql
-- Enable Write-Ahead Logging for better concurrency
PRAGMA journal_mode=WAL;

-- Increase cache size (10MB default → 100MB)
PRAGMA cache_size=-100000;

-- Synchronous mode for performance (NORMAL is safe with WAL)
PRAGMA synchronous=NORMAL;

-- Memory-mapped I/O for faster reads
PRAGMA mmap_size=268435456;  -- 256MB

-- Analyze tables for query planner
ANALYZE;
```

### PostgreSQL Optimizations

```sql
-- Increase shared buffers (default 128MB → 256MB)
shared_buffers = 256MB

-- Increase work_mem for sorting/hashing
work_mem = 16MB

-- Enable parallel query execution
max_parallel_workers_per_gather = 2

-- Vacuum regularly to reclaim space
VACUUM ANALYZE kv_store;

-- Update statistics
ANALYZE kv_store;
```

## Usage Examples

### Basic Operations

```rust
use plexspaces_keyvalue::{KeyValueStore, SqlKVStore};

let store = SqlKVStore::new("sqlite:kv.db").await?;

// Put
store.put("actor:counter", b"actor_ref_data".to_vec(), None).await?;

// Get
let value = store.get("actor:counter").await?;

// List with prefix
let actors = store.list("actor:").await?;

// Delete
store.delete("actor:counter").await?;
```

### TTL Support

```rust
// Put with TTL (expires in 60 seconds)
store.put("session:user123", data, Some(Duration::from_secs(60))).await?;

// TTL cleanup runs automatically in background
```

### Batch Operations

```rust
// Batch put
let mut batch = vec![];
batch.push(("key1".to_string(), b"value1".to_vec(), None));
batch.push(("key2".to_string(), b"value2".to_vec(), None));
store.batch_put(batch).await?;
```

## Backend Support

### SQLite

- **Use Case**: Single-node, embedded storage
- **Performance**: Fast for small to medium datasets
- **Limitations**: Not distributed, write concurrency limited

### PostgreSQL

- **Use Case**: Multi-node, distributed storage
- **Performance**: Excellent for large datasets, high concurrency
- **Features**: ACID transactions, replication, sharding

### Redis

- **Use Case**: High-performance caching, distributed storage
- **Performance**: < 1ms latency, high throughput
- **Features**: Pub/sub, TTL, clustering

## Monitoring & Metrics

```rust
// Query latency histogram
metrics::histogram!("kv_query_duration_ms",
    "operation" => "get|put|list|delete",
    "backend" => "sqlite|postgres"
);

// Slow query counter
metrics::counter!("kv_slow_query_total",
    "operation" => "get|put|list|delete",
    "threshold_ms" => "10"
);

// Index hit ratio
metrics::gauge!("kv_index_hit_ratio",
    "index" => "pk|ttl|prefix"
);
```

## References

- Implementation: `crates/keyvalue/src/`
- Performance Analysis: See performance benchmarks in code
- Tests: `crates/keyvalue/src/` (unit tests)

