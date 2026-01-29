# Financial Risk Assessment Example

## Overview

This example demonstrates a loan application workflow using PlexSpaces actors, showcasing:

- **Multi-step workflow orchestration** (data collection, risk scoring, decision, post-decision)
- **Worker pools** for parallel processing
- **ConfigBootstrap** for Erlang/OTP-style configuration
- **CoordinationComputeTracker** for metrics tracking
- **NodeBuilder/ActorBuilder** for simplified actor setup

## Architecture

```
LoanCoordinator
  ├─> CreditCheckWorker Pool (5 workers)
  ├─> BankAnalysisWorker Pool (5 workers)
  ├─> EmploymentWorker Pool (3 workers)
  ├─> RiskScoringWorker Pool (2 workers)
  ├─> DecisionEngineWorker (1 worker)
  ├─> DocumentWorker Pool (2 workers)
  ├─> NotificationWorker Pool (3 workers)
  └─> AuditWorker (1 worker)
```

## Workflow Steps

1. **Data Collection** (parallel):
   - Credit Check (simulated credit bureau API)
   - Bank Analysis (simulated Plaid API)
   - Employment Verification (simulated The Work Number API)

2. **Risk Scoring**:
   - ML model inference (simulated)
   - Fraud detection
   - Default probability calculation

3. **Decision Logic**:
   - Auto-approve (score > 750)
   - Manual review (600 < score ≤ 750)
   - Auto-reject (score ≤ 600)

4. **Post-Decision Actions**:
   - Document generation
   - Email notifications
   - Audit logging

## Framework Abstractions Used

| Abstraction | Usage |
|-------------|-------|
| **NodeBuilder** | Simplified node creation |
| **ActorBuilder** | Simplified actor creation with mailbox configuration |
| **ConfigBootstrap** | Erlang/OTP-style configuration loading |
| **CoordinationComputeTracker** | Metrics tracking (coordination vs compute) |
| **ActorBehavior** | All workers implement this trait |
| **Message** | Request-reply pattern for worker communication |

## Configuration

Configuration is loaded from `release.toml` using `ConfigBootstrap`:

```toml
[finance_risk.worker_pools]
credit_check = 5
bank_analysis = 5
employment = 3
risk_scoring = 2
decision_engine = 1
document = 2
notification = 3
audit = 1

[finance_risk.node]
node_id = "finance-risk-node"
listen_addr = "0.0.0.0:9000"

[finance_risk]
backend = "memory"  # Options: memory, redis, sqlite
```

Environment variables can override config values.

## Running the Example

### Quick Start

```bash
cd examples/finance-risk
./scripts/run.sh
```

### Run Tests

```bash
./scripts/run_tests.sh
```

### Manual Run

```bash
cargo run --release --bin finance-coordinator
```

## Metrics

The example uses `CoordinationComputeTracker` to track:
- **Coordination Time**: Message passing, actor spawning
- **Compute Time**: Actual work (credit checks, risk scoring, etc.)

Example output:
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Coordination vs Compute Metrics
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Coordination Time: 150ms
Compute Time: 2000ms
Granularity Ratio: 13.33×
```

## Code Structure

```
examples/finance-risk/
├── Cargo.toml
├── release.toml              # Configuration
├── README.md                 # This file
├── scripts/
│   ├── run.sh               # Run example
│   └── run_tests.sh         # Run tests
└── src/
    ├── main.rs              # Main entry point
    ├── lib.rs               # Library exports
    ├── config.rs            # Configuration structures
    ├── coordinator.rs       # LoanCoordinator actor
    ├── models.rs            # Data models
    ├── workers/             # Worker actors
    │   ├── credit_check_worker.rs
    │   ├── bank_analysis_worker.rs
    │   ├── employment_worker.rs
    │   ├── risk_scoring_worker.rs
    │   ├── decision_engine_worker.rs
    │   ├── document_worker.rs
    │   ├── notification_worker.rs
    │   └── audit_worker.rs
    └── bin/
        └── test_durability.rs  # Durability test
```

## Testing

```bash
# Run unit tests
cargo test --lib

# Run integration tests
cargo test --test '*'

# Run with clippy
cargo clippy -- -D warnings

# Check formatting
cargo fmt --check
```

## Notes

- This is a simplified example demonstrating the workflow pattern
- In production, the coordinator would orchestrate the full workflow across all worker pools
- External API calls are simulated (credit bureaus, Plaid, etc.)
- ML model inference is simulated
- For durability, see `test_durability.rs` example
