# Elastic Pool - Auto-Scaling Worker Pools

**Purpose**: Provides auto-scaling worker pools for microservices, inspired by Erlang poolboy and database connection pools. Combines actors with dynamic scaling based on load.

## Overview

ElasticPool is built on PlexSpaces core components:
- **Supervisor**: Manages worker actors with restart policies
- **Channel**: Work queue for distributing requests
- **Auto-scaler**: Monitors load and adjusts pool size dynamically

## Usage Examples

### Basic Usage

```rust
use plexspaces_elastic_pool::*;
use plexspaces_proto::pool::v1::*;

// Configure pool
let config = PoolConfig {
    min_size: 2,
    max_size: 10,
    scale_up_threshold: 0.8,
    scale_down_threshold: 0.3,
    ..Default::default()
};

let pool = ElasticPool::new(config, worker_behavior).await?;

// Checkout worker
let worker = pool.checkout().await?;

// Use worker
worker.process_task(task).await?;

// Return worker to pool
pool.checkin(worker).await?;
```

## Dependencies

This crate depends on:
- `plexspaces_proto`: Protocol buffer definitions
- `plexspaces_channel`: MPSC channel for checkout queue
- `plexspaces_actor`: Actor abstraction for workers
- `plexspaces_supervisor`: Supervisor for worker lifecycle

## Dependents

This crate is used by:
- Applications: Pool of payment service clients, database connections, etc.

## References

- Implementation: `crates/elastic-pool/src/`
- Tests: `crates/elastic-pool/tests/`
- Proto definitions: `proto/plexspaces/v1/pool.proto`
- Inspiration: Erlang poolboy, database connection pools

