# Data Parallel Worker App

**WASM-deployable worker actor** for data-parallel processing using the ShardGroup pattern.

## Overview

This app provides worker actors that can be deployed as WASM to PlexSpaces nodes. Once deployed, the worker behavior is registered on the node, allowing ShardGroups to spawn worker actors for parallel processing.

## Quick Start

### Build

```bash
cd examples/rust/apps/data_parallel_worker
cargo build
```

### Deploy to Nodes

```bash
# Build WASM (when WASM target is ready)
cargo build --target wasm32-wasip2

# Deploy to node using HTTP API
curl -X POST http://localhost:8001/api/v1/applications/deploy \
  -F "application_id=data-parallel-worker" \
  -F "name=data-parallel-worker" \
  -F "version=1.0.0" \
  -F "wasm_file=@target/wasm32-wasip2/release/data_parallel_worker.wasm"
```

### Use with ShardGroup

Once deployed, create a ShardGroup that uses the worker behavior:

```rust
use plexspaces_sdk::{ShardGroupClient, ShardGroupClientTrait};

let mut client = ShardGroupClient::connect_grpc("http://localhost:8000").await?;

client.create_shard_group(
    "worker-pool-1".to_string(),
    "worker".to_string(),
    4,
    PartitionStrategy::PartitionStrategyHash,
    None,
).await?;
```

## Architecture

- **WorkerActor**: Processes data-parallel tasks (increment, set, get, stats)
- **Behavior Registration**: Registers "worker" behavior on the node
- **ShardGroup Integration**: Works with ShardGroup for parallel processing

## Testing

See `examples/rust/apps/data_parallel_worker/scripts/test.sh` for deployment and testing instructions.
