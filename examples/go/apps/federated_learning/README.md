# Federated Learning Simulation (Go)

Privacy-preserving federated learning — Go WASM implementation.

Multiple client actors train locally on disjoint data partitions, push gradients to a central aggregator that applies FedAvg with differential privacy (Gaussian noise), broadcasts updated weights, and tracks convergence.

## Features

- Logistic regression (10 features) — all pure Go math, no external ML packages
- FedAvg aggregation with weighted client contributions
- Differential privacy: Gaussian noise via Box-Muller, gradient clipping, configurable epsilon
- Non-IID data: each client gets shifted feature distributions
- Convergence tracking with early stopping
- Compute vs coordination metrics tracking

## Architecture

| Actor | Role | Responsibilities |
|-------|------|-----------------|
| AggregatorActor | aggregator | Manage training rounds, broadcast weights, FedAvg + DP, convergence |
| ClientActor | client | Local training, gradient computation, noise addition |

## Quick Start

```bash
./build.sh
./test.sh 8091 8094
./undeploy.sh
```

## References

- [Python version](../../../python/apps/federated_learning/)
- [Architecture](../../../../docs/architecture.md)
