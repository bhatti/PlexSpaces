# Byzantine Generals Example - Distributed Consensus with Fault Tolerance

## Overview

This example demonstrates the **Byzantine Generals Problem**, a classic distributed systems challenge that showcases PlexSpaces' capabilities:

- **Distributed actor messaging** across nodes
- **TupleSpace coordination** for decoupled communication
- **Fault tolerance** with supervision
- **Consensus algorithms** in actor systems
- **ConfigBootstrap** for Erlang-style configuration
- **NodeBuilder/ActorBuilder** for simplified actor creation
- **CoordinationComputeTracker** for performance metrics

## The Byzantine Generals Problem

### Problem Statement

Several generals surround an enemy city and must coordinate an attack:
- All loyal generals must agree on a common plan (attack or retreat)
- Some generals may be **traitors** sending conflicting messages
- Communication is only through messengers
- Must reach consensus despite traitors

### Formal Requirements

1. **Agreement**: All loyal generals decide on the same plan
2. **Validity**: If all loyal generals propose the same plan, that plan is chosen
3. **Termination**: All generals eventually decide
4. **Fault Tolerance**: Works with up to ⌊(n-1)/3⌋ traitors (n = total generals)

**Example**:
- 4 generals total → can tolerate 1 traitor
- 7 generals total → can tolerate 2 traitors
- 10 generals total → can tolerate 3 traitors

## Architecture

### System Diagram

```
┌──────────────────────────────────────────────────────────┐
│  Byzantine Consensus System                               │
│                                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐      │
│  │ General1    │  │ General2    │  │ General3    │      │
│  │ (Node1)     │  │ (Node2)     │  │ (Node3)     │      │
│  │ Loyal       │  │ Loyal       │  │ TRAITOR     │      │
│  └─────────────┘  └─────────────┘  └─────────────┘      │
│         │                 │                 │             │
│         └─────────────────┴─────────────────┘             │
│                           │                               │
│                           ▼                               │
│                  ┌──────────────────┐                     │
│                  │  TupleSpace      │                     │
│                  │  (Shared Redis)  │                     │
│                  │                  │                     │
│                  │  Messages:       │                     │
│                  │  Votes:          │                     │
│                  │  Results:        │                     │
│                  └──────────────────┘                     │
└──────────────────────────────────────────────────────────┘
```

### Components

#### 1. **General Actor** (`src/byzantine.rs`)

Each general is an actor that:
- Proposes initial vote (if commander)
- Collects votes from other generals
- Runs consensus algorithm (Byzantine fault tolerance)
- Makes final decision

**State**:
```rust
struct GeneralState {
    id: String,
    is_commander: bool,
    is_faulty: bool,
    round: u32,
    votes: HashMap<String, Vec<Vote>>,
    decision: Decision,
    message_count: usize,
}
```

**Messages**:
- `PROPOSE` - Commander's proposal
- `VOTE` - Vote from another general
- `DECIDE` - Trigger decision making

#### 2. **ByzantineApplication** (`src/application.rs`)

Implements the `Application` trait:
- Uses NodeBuilder/ActorBuilder for actor creation
- Creates N generals (1 commander, N-1 lieutenants)
- F generals are Byzantine (faulty)
- Uses TupleSpace for coordination
- Tracks coordination vs compute metrics

#### 3. **TupleSpace Coordination**

Votes are coordinated via TupleSpace using structured tuples:
- **Proposal Tuple**: `["proposal", from_general, vote, round]`
- **Vote Tuple**: `["vote", from_general, to_general, vote, round, path]`
- **Decision Tuple**: `["decision", general_id, final_vote]`

## Consensus Algorithm

### Simplified Byzantine Fault Tolerance (BFT)

#### **Phase 1: Proposal**
1. Commander proposes initial vote to TupleSpace
2. All generals read proposals

#### **Phase 2: Voting**
3. Each general sends vote to every other general based on majority
4. Each general collects votes addressed to them

#### **Phase 3: Decision**
5. Each general decides based on majority of votes
6. Commander verifies consensus

### Traitor Behavior

Traitors can:
- Send different votes to different generals
- Send random votes
- Not vote at all
- Change votes between rounds

## Configuration

### Using ConfigBootstrap

Configuration is loaded from `release.toml`:

```toml
[byzantine]
general_count = 4
fault_count = 1
tuplespace_backend = "memory"
redis_url = ""
postgres_url = ""
```

Or via environment variables:
```bash
export BYZANTINE_GENERAL_COUNT=4
export BYZANTINE_FAULT_COUNT=1
```

### Configuration Options

- `general_count`: Total number of generals (minimum 4)
- `fault_count`: Number of Byzantine generals (must be < general_count/3)
- `tuplespace_backend`: Backend type ("memory", "redis", "postgres")
- `redis_url`: Redis connection string (if using redis backend)
- `postgres_url`: PostgreSQL connection string (if using postgres backend)

## Running the Example

### Quick Start (Recommended)

```bash
cd examples/byzantine

# Run with scripts (shows metrics)
./scripts/run.sh

# Or run directly
cargo run --release --bin byzantine
```

### Using Scripts

The example includes several scripts for different use cases:

#### Run Example with Metrics
```bash
./scripts/run.sh
```

This will:
- Build the example in release mode
- Run the consensus simulation
- Display metrics (coordination vs compute time, granularity ratio)

#### Run All Tests
```bash
./scripts/run_tests.sh
```

This will:
- Run unit tests
- Run integration tests
- Build binary
- Run example with metrics
- Display test summary

#### Run E2E Tests with Detailed Metrics
```bash
./scripts/run_e2e.sh
```

This will:
- Run end-to-end application tests
- Display detailed metrics
- Show coordination vs compute breakdown

### Single-Process (In-Memory)

```bash
cd examples/byzantine
cargo run --release --bin byzantine
```

### Multi-Process (Redis Backend)

```bash
# Terminal 1: Start Redis
docker run -p 6379:6379 redis:7

# Terminal 2: Node 1
cargo run --release --bin byzantine_node -- \
  --node-id node1 \
  --address localhost:8000 \
  --tuplespace-backend redis \
  --redis-url redis://localhost:6379 \
  --generals general0,general1

# Terminal 3: Node 2
cargo run --release --bin byzantine_node -- \
  --node-id node2 \
  --address localhost:8001 \
  --tuplespace-backend redis \
  --redis-url redis://localhost:6379 \
  --generals general2,general3
```

## Metrics

The example uses `CoordinationComputeTracker` to track performance metrics:

### Metrics Displayed

When running the example, you'll see:

```
📊 Metrics:
   Coordination time: 12.34 ms
   Compute time: 45.67 ms
   Granularity ratio: 3.70
```

### Metrics Explained

- **Coordination time**: Time spent in TupleSpace operations (vote exchange, barrier synchronization)
- **Compute time**: Time spent in consensus algorithm computation
- **Granularity ratio**: `compute_time / coordinate_time` (target: > 1.0 for good performance)

### Expected Metrics

For a typical run with 4 generals:
- Coordination time: 10-50ms (depends on TupleSpace backend)
- Compute time: 5-20ms (consensus algorithm)
- Granularity ratio: 0.5-2.0 (for small examples, coordination may dominate)

For larger examples (7+ generals), compute time should dominate coordination time.

## Testing

### Run All Tests

```bash
# Using script (recommended)
./scripts/run_tests.sh

# Or directly
cargo test
```

### Test Suites

- `basic_consensus` - Single-node consensus tests
- `distributed_consensus` - Multi-node consensus tests
- `multi_process_consensus` - Multi-process tests with Redis
- `supervised_consensus` - Supervision and fault tolerance tests
- `e2e_application` - End-to-end application tests

### Example Test

```bash
# Test 4 generals with 1 traitor
cargo test --test basic_consensus test_4_generals_1_traitor

# Run E2E tests with metrics
./scripts/run_e2e.sh
```

### Integration Testing with Redis

For distributed testing, you'll need Redis:

```bash
# Start Redis
docker run -p 6379:6379 redis:7

# Run distributed tests
cargo test --features redis-backend --test distributed_consensus
```

## Key Features Demonstrated

1. **ConfigBootstrap**: Erlang-style configuration loading from `release.toml`
2. **NodeBuilder/ActorBuilder**: Simplified actor creation patterns
3. **CoordinationComputeTracker**: Performance metrics tracking
4. **Application Trait**: Lifecycle management
5. **Supervision**: Fault tolerance via automatic restart
6. **TupleSpace**: Decoupled coordination for vote exchange
7. **ActorBehavior**: Message-based actor communication

## Success Criteria

1. ✅ **Correctness**: All loyal generals agree
2. ✅ **Fault Tolerance**: Works with ⌊(n-1)/3⌋ traitors
3. ✅ **Termination**: Completes in bounded time
4. ✅ **Distribution**: Works across multiple nodes
5. ✅ **Recovery**: Handles node failures
6. ✅ **Scalability**: Scales to 7-10 generals

## Implementation Details

### Message Flow

1. Commander sends `PROPOSE` message with initial vote
2. All generals receive proposal and cast votes via TupleSpace
3. Generals read votes from TupleSpace
4. Each general decides based on majority
5. Consensus is verified

### TupleSpace Tuple Formats

**Proposal**:
```
["proposal", "general-0", "Attack", 0]
```

**Vote**:
```
["vote", "general-1", "general-0", "Attack", 0, "general-1"]
```

**Decision**:
```
["decision", "general-0", "Attack"]
```

## References

- [Byzantine Generals Problem](https://en.wikipedia.org/wiki/Byzantine_fault)
- [Lamport, Shostak, Pease (1982)](https://www.microsoft.com/en-us/research/uploads/prod/2016/12/The-Byzantine-Generals-Problem.pdf)
- PlexSpaces Documentation: `docs/`
