# Ray Parameter Server - Distributed ML Training (Python WASM with SDK)

Demonstrates **distributed ML training** with the **parameter server pattern** (Ray-style).

**Real-world use case**: Distributed ML training systems (TensorFlow, PyTorch, Ray) where:
- Centralized parameter server manages model weights
- Elastic worker pools compute gradients in parallel
- Gradients are aggregated and applied synchronously or asynchronously
- Workers can scale horizontally based on dataset size

## Overview

This example implements the Ray parameter server pattern for distributed ML training:

1. **Parameter Server Actor**: Manages centralized model weights, aggregates gradients from workers
2. **Data Worker Actors**: Compute gradients on data shards in parallel
3. **Synchronous Training**: All workers compute gradients before parameter server updates weights

## PlexSpaces Python SDK

This example uses the [PlexSpaces Python SDK](../../../../sdks/python/README.md):

```python
from plexspaces import actor, state, handler, init_handler, host

@actor
class ParameterServer:
    w1: List[List[float]] = state(default_factory=lambda: [[0.1] * 784 for _ in range(200)])
    w2: List[float] = state(default_factory=lambda: [0.1] * 200)
    learning_rate: float = state(default=0.01)
    iteration: int = state(default=0)
    
    @handler("apply_gradients")
    def apply_gradients(self, gradients: List[Dict[str, Any]] = None) -> dict:
        # Aggregate and apply gradients from multiple workers
        ...
```

**Before SDK**: 500+ lines with manual WIT interface  
**After SDK**: ~200 lines with decorators

## Quick Start

### 1. Build

```bash
./build.sh
```

This builds a single WASM application containing both actor types:
- `ray_actors.wasm` - Contains both `ParameterServer` and `DataWorker` actor classes

### 2. Start PlexSpaces Node

```bash
# Terminal 1: Start node
cd /path/to/plexspaces
./scripts/server.sh
# Or manually:
cargo run -p plexspaces-cli -- start --node-id test-node --listen-addr 0.0.0.0:8093
```

### 3. Run Training

```bash
# Terminal 2: Run training test
./test.sh 8094  # HTTP gateway port (gRPC 8093 + 1)
```

## Architecture

### Parameter Server Pattern

```
┌─────────────────┐
│ Parameter Server│  ← Centralized model weights
│   (Weights)     │
└────────┬────────┘
         │
    ┌────┴────┐
    │         │
┌───▼───┐ ┌───▼───┐
│Worker │ │Worker │  ← Compute gradients in parallel
│   0   │ │   1   │
└───┬───┘ └───┬───┘
    │         │
    └────┬────┘
         │
    ┌────▼────┐
    │Gradients│  ← Aggregated and applied
    └─────────┘
```

### Synchronous Training Flow

1. **Get Weights**: Parameter server returns current model weights
2. **Compute Gradients**: All workers compute gradients in parallel on their data shards
3. **Aggregate Gradients**: Parameter server aggregates gradients from all workers
4. **Update Weights**: Parameter server applies aggregated gradients to update weights
5. **Repeat**: Steps 1-4 for each training iteration

## API Reference

### Parameter Server Actor

| Handler | Description | Payload |
|---------|-------------|---------|
| `get_weights` | Get current model weights | `{}` |
| `apply_gradients` | Apply gradients from workers | `{"gradients": [{"d_w1": [...], "d_w2": [...]}, ...]}` |
| `stats` | Get training statistics | `{}` |

### Data Worker Actor

| Handler | Description | Payload |
|---------|-------------|---------|
| `compute_gradients` | Compute gradients on data shard | `{"weights": {"w1": [...], "w2": [...]}}` |
| `stats` | Get worker statistics | `{}` |

## Metrics & Benchmarks

The test script tracks:

- **Coordination Time**: Message passing, API calls between actors
- **Computation Time**: Actual gradient computation on data shards
- **Granularity Ratio**: `compute_time / coordinate_time` (should be >= 10x)
- **Efficiency**: `compute_time / total_time` (closer to 1.0 is better)
- **Message Count**: Total messages sent during training

### Example Output

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
📈 Coordination vs Computation Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Coordination Time: 450ms (message passing, API calls)
Computation Time:  3200ms (gradient computation)
Total Messages:    50
Granularity Ratio: 7.11 (compute/coordinate)
Efficiency:        87.7% (compute/total)
Coordination %:    12.3%
```

## Comparison: Ray vs PlexSpaces

| Feature | Ray (Native Python) | PlexSpaces (Python WASM) |
|---------|---------------------|--------------------------|
| **Language** | Python | Python (WASM) |
| **Actor Model** | `@ray.remote` decorator | `@actor` decorator |
| **State Management** | Class attributes | `state()` decorator |
| **Message Passing** | Method calls (`.remote()`) | HTTP API calls |
| **Distribution** | Built-in Ray cluster | gRPC + HTTP gateway |
| **Elastic Scaling** | Built-in | Manual (add/remove workers) |
| **Resource Scheduling** | Ray scheduler | PlexSpaces placement service |

### Ray Code (Native)

```python
@ray.remote
class ParameterServer:
    def __init__(self, lr):
        self.w1 = np.random.randn(200, 784) * 0.1
        self.w2 = np.random.randn(200) * 0.1
    
    def apply_gradients(self, *gradients):
        # Aggregate and update weights
        ...

# Usage
ps = ParameterServer.remote(0.01)
workers = [DataWorker.remote(...) for _ in range(4)]
gradients = [w.compute_gradients.remote(weights) for w in workers]
ps.apply_gradients.remote(*ray.get(gradients))
```

### PlexSpaces Code (Python WASM)

```python
@actor
class ParameterServer:
    w1: List[List[float]] = state(default_factory=lambda: [[0.1] * 784 for _ in range(200)])
    
    @handler("apply_gradients")
    def apply_gradients(self, gradients: List[Dict] = None) -> dict:
        # Aggregate and update weights
        ...

# Usage (via HTTP API)
curl -X POST "http://localhost:8094/api/v1/actors/ray-ps/parameter-server" \
    -d '{"op":"apply_gradients","gradients":[...]}'
```

## Design Decisions

### Why Parameter Server Pattern?

- **Centralized Weights**: Single source of truth for model parameters
- **Gradient Aggregation**: Combine gradients from multiple workers efficiently
- **Synchronous/Asynchronous**: Support both training patterns
- **Industry Standard**: Used by TensorFlow, PyTorch, Ray

### Why Elastic Worker Pools?

- **Horizontal Scaling**: Add/remove workers based on dataset size
- **Data Parallelism**: Each worker processes a shard of the dataset
- **Resource-Aware**: Distribute computation based on worker availability
- **Fault Tolerance**: Failed workers don't block entire training

### Why Actor Model?

- **Stateful Server**: Parameter server maintains model weights
- **Message Passing**: Workers send gradients, server sends weights
- **Location Transparency**: Works across multiple nodes
- **Natural Fit**: Matches Ray's actor-based parameter server pattern

## Files

| File | Description |
|------|-------------|
| `ray_actors.py` | Both actor classes (ParameterServer and DataWorker) in one file |
| `build.sh` | Build script for single WASM application |
| `test.sh` | Training test script with metrics |
| `app-config.toml` | Single ApplicationSpec deploying both actor types |
| `native/parameter_server.py` | Native Ray reference implementation |

## SDK Features Demonstrated

| Feature | How It's Used |
|---------|---------------|
| `@actor` | Marks `ParameterServer` and `DataWorker` as PlexSpaces actors |
| `state()` | Defines persistent state (weights, iteration, learning_rate) |
| `@handler()` | Routes `get_weights`, `apply_gradients`, `compute_gradients` |
| `@init_handler` | Initializes actors from config |
| `host.info()` | Logging for debugging |

## See Also

- [PlexSpaces Python SDK](../../../../sdks/python/README.md)
- [Ray Documentation](https://docs.ray.io/)
- [Parameter Server Pattern](https://docs.ray.io/en/latest/ray-core/examples/plot_parameter_server.html)
- [Examples Overview](../../README.md)
