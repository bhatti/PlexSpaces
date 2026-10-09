# Federated Learning Simulation (Python)

Privacy-preserving distributed ML training -- Python WASM implementation.

Client actors train on local data partitions, send clipped+noised gradients to a central aggregator that applies FedAvg with differential privacy (Gaussian mechanism), broadcasts updated weights, and tracks convergence.

## Architecture

- **AggregatorActor** (leader): Manages training rounds, broadcasts weights, collects and averages gradients with DP noise, tracks convergence
- **ClientActor** (worker): Receives weights, trains locally on private data partition, computes and clips gradients, sends back to aggregator

## Model

- Logistic regression with 10 features (pure Python, no numpy/sklearn)
- Binary classification on synthetic dataset
- Each client gets a disjoint data partition

## Privacy Guarantees

- Gradient clipping (L2 norm bound)
- Gaussian noise mechanism (server-side)
- Composition theorem for multi-round privacy budget
- Configurable epsilon/delta

## PlexSpaces Primitives Used

| Primitive | Usage |
|-----------|-------|
| `@actor` / `@handler` | Actor definition and message dispatch |
| `ACTOR_ROLES` | Multi-role dispatch (aggregator + client) |
| `host.create_shard_group()` | Client worker pool |
| `host.scatter_gather()` | Distribute training rounds to all clients |
| `host.application_metrics_add()` | Compute vs coordination tracking |

## Benchmarks

- Rounds to convergence
- Gradient aggregation latency
- Compute (local training) vs coordination (gradient exchange) breakdown
- Strong scaling with client count [2, 4, 8, 16]

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Flower](https://flower.ai/) -- Production federated learning framework
- [PySyft](https://github.com/OpenMined/PySyft) -- Privacy-preserving ML
- [Architecture](../../../../docs/architecture.md)
