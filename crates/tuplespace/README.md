# TupleSpace - Linda-Style Coordination

**Purpose**: Provides distributed shared memory abstraction with pattern matching for actor coordination.

## Overview

TupleSpace implements the Linda coordination model, enabling decoupled communication between actors through a shared space of tuples. Actors can write, read, and take tuples using pattern matching.

## Key Components

### TupleSpace

Main tuplespace implementation:

```rust
pub struct TupleSpace {
    storage: Arc<dyn TupleSpaceStorage>,
    tenant_id: String,
    namespace: String,
}
```

### Tuple

A tuple is a vector of fields:

```rust
pub struct Tuple {
    fields: Vec<TupleField>,
}

pub enum TupleField {
    String(String),
    Integer(i64),
    Float(OrderedFloat<f64>),
    Boolean(bool),
    Bytes(Vec<u8>),
}
```

### Pattern

Pattern matching for tuple queries:

```rust
pub struct Pattern {
    fields: Vec<PatternField>,
}

pub enum PatternField {
    Exact(TupleField),
    Wildcard,
}
```

## Operations

### Write

Write a tuple to the space:

```rust
let tuple = Tuple::new(vec![
    TupleField::String("result".to_string()),
    TupleField::Integer(42),
]);
tuplespace.write(tuple).await?;
```

### Read

Read tuples matching a pattern (non-destructive):

```rust
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("result".to_string())),
    PatternField::Wildcard,
]);
let tuples = tuplespace.read(pattern).await?;
```

### Take

Take tuples matching a pattern (destructive):

```rust
let tuples = tuplespace.take(pattern).await?;
```

## Storage Backends

### In-Memory

```rust
let tuplespace = TupleSpace::new();
```

### SQLite

```rust
let tuplespace = TupleSpace::with_sqlite_backend(":memory:").await?;
```

### PostgreSQL

```rust
let tuplespace = TupleSpace::with_postgres_backend("postgresql://localhost/plexspaces").await?;
```

### Redis

```rust
let tuplespace = TupleSpace::with_redis_backend("redis://localhost:6379").await?;
```

## Usage Examples

### Basic Coordination

```rust
use plexspaces_tuplespace::{TupleSpace, Tuple, Pattern, TupleField, PatternField};

let tuplespace = TupleSpace::new();

// Producer writes result
let result = Tuple::new(vec![
    TupleField::String("calculation".to_string()),
    TupleField::String("add".to_string()),
    TupleField::Integer(5),
    TupleField::Integer(3),
    TupleField::Integer(8),
]);
tuplespace.write(result).await?;

// Consumer reads result
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("calculation".to_string())),
    PatternField::Wildcard,  // operation
    PatternField::Wildcard,  // operand1
    PatternField::Wildcard,  // operand2
    PatternField::Wildcard,  // result
]);
let tuples = tuplespace.read(pattern).await?;
```

## Design Principles

### Linda Model

- **Associative addressing**: Find tuples by content, not address
- **Time uncoupling**: Producers and consumers don't need to exist simultaneously
- **Space uncoupling**: Producers and consumers don't need to know each other

### Multi-Tenancy

Tuplespaces are scoped by tenant and namespace:

```rust
let tuplespace = TupleSpace::with_tenant_namespace("acme-corp", "production");
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_keyvalue`: Key-value storage backend
- `sqlx`: SQL backends (optional)
- `redis`: Redis backend (optional)

## Dependents

This crate is used by:
- `plexspaces_node`: Node provides TupleSpace to actors
- `plexspaces_core`: ActorContext includes TupleSpaceProvider
- `plexspaces_tuplespace_service`: gRPC service wrapper

## References

- [PlexSpaces Architecture](../../docs/architecture.md) - System design overview
- [Detailed Design - TupleSpace](../../docs/detailed-design.md#tuplespace) - Comprehensive TupleSpace documentation
- [Getting Started Guide](../../docs/getting-started.md) - Quick start with TupleSpace
- Implementation: `crates/tuplespace/src/`
- Tests: `crates/tuplespace/tests/`
- Proto definitions: `proto/plexspaces/v1/tuplespace.proto`
- Inspiration: Linda coordination model

