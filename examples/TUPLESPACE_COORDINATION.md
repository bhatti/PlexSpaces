# TupleSpace Coordination Patterns for Data Parallel Computing

## Purpose

This document explains how PlexSpaces uses TupleSpace for **data parallel coordination** in high-performance computing (HPC) scenarios. It demonstrates patterns from two examples:
1. **Byzantine Generals**: Distributed consensus (fault tolerance)
2. **Heat Diffusion**: Scientific computing (iterative solvers)

Both examples showcase **multi-node coordination** via TupleSpace with registry-based discovery.

---

## Core Principle: Linda Coordination Model

PlexSpaces TupleSpace is based on the **Linda coordination model**:
- **Tuple**: Ordered list of typed fields `("tag", value1, value2, ...)`
- **Write**: Add tuple to shared space
- **Read**: Match pattern, return tuple (non-destructive)
- **Take**: Match pattern, remove tuple (destructive)
- **Pattern Matching**: Wildcards + exact matches for flexible queries

**Key Insight**: TupleSpace provides **spatial and temporal decoupling**:
- Producers don't need to know consumers (spatial)
- Producers can write before consumers exist (temporal)

---

## Pattern 1: Distributed Consensus (Byzantine Generals)

### Use Case
Multiple nodes reaching agreement despite failures (distributed voting).

### Architecture
```
┌────────────────────────────────────────────────────────────┐
│              TupleSpaceRegistry                            │
│              (Service Discovery)                           │
│                                                            │
│  Registered: ts-sqlite-army1-consensus                     │
│  - tenant: "army1", namespace: "consensus"                 │
│  - backend: SQLite                                         │
│  - status: HEALTHY                                         │
└───────────────┬────────────────────────────────────────────┘
                │
                │ discover(tenant="army1", namespace="consensus")
    ┌───────────┴───────────┐
    │                       │
┌───▼────┐              ┌───▼────┐
│ Node 1 │              │ Node 2 │
│        │              │        │
│ Gen0   │              │ Gen2   │
│ Gen1   │              │ Gen3   │
└───┬────┘              └───┬────┘
    │                       │
    │   votes via TupleSpace │
    └───────────┬────────────┘
                ▼
     ┌──────────────────────┐
     │ Distributed          │
     │ TupleSpace           │
     │ (SQLite Backend)     │
     │                      │
     │ Tuples:              │
     │ - ("proposal", ...)  │
     │ - ("vote", gen0, ..) │
     │ - ("vote", gen1, ..) │
     │ - ("vote", gen2, ..) │
     │ - ("vote", gen3, ..) │
     └──────────────────────┘
```

### Coordination Flow

#### Phase 1: Service Discovery
```rust
// Both nodes discover the same TupleSpace
let registry = Arc::new(TupleSpaceRegistry::new(kv_store));

// Register TupleSpace for "army1" tenant, "consensus" namespace
registry.register(
    "ts-sqlite-army1-consensus",
    "army1",      // tenant (multi-tenancy isolation)
    "consensus",  // namespace (coordination scope)
    "localhost:9001",
    "sqlite",
    capabilities,
).await?;

// Node 1 discovers
let tuplespace_node1 = DiscoveredTupleSpace::discover(
    registry.clone(), "army1", "consensus", shared_tuplespace.clone()
).await?;

// Node 2 discovers SAME TupleSpace
let tuplespace_node2 = DiscoveredTupleSpace::discover(
    registry.clone(), "army1", "consensus", shared_tuplespace.clone()
).await?;
```

**Key Point**: Both nodes discover and connect to the **same distributed TupleSpace instance**.

#### Phase 2: Proposal Broadcasting
```rust
// Commander proposes Attack via TupleSpace
let proposal = Tuple::new(vec![
    TupleField::String("proposal".to_string()),
    TupleField::Integer(0),  // round
    TupleField::String("Attack".to_string()),
    TupleField::String("general0".to_string()),  // commander
]);
tuplespace.write(proposal).await?;
```

#### Phase 3: Cross-Node Reading
```rust
// All generals (across all nodes) read the same proposal
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("proposal".to_string())),
    PatternField::Wildcard,  // any round
    PatternField::Wildcard,  // any decision
    PatternField::Wildcard,  // any commander
]);

let proposals = tuplespace.read_all(pattern).await?;
// General on Node 1 sees proposal written by general on Node 1
// General on Node 2 ALSO sees the same proposal (cross-node visibility)
```

#### Phase 4: Vote Coordination
```rust
// Each general casts vote via TupleSpace
let vote = Tuple::new(vec![
    TupleField::String("vote".to_string()),
    TupleField::Integer(0),  // round
    TupleField::String("general0".to_string()),  // voter
    TupleField::String("Attack".to_string()),  // decision
    TupleField::String(serde_json::to_string(&path)?),  // signature chain
]);
tuplespace.write(vote).await?;

// All generals read all votes (cross-node visibility)
let vote_pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("vote".to_string())),
    PatternField::Exact(TupleField::Integer(0)),  // round 0
    PatternField::Wildcard,  // any voter
    PatternField::Wildcard,  // any decision
    PatternField::Wildcard,  // any path
]);

let votes = tuplespace.read_all(vote_pattern).await?;
// General on Node 1: sees all 4 votes (2 from Node 1, 2 from Node 2)
// General on Node 2: sees all 4 votes (same global view)
```

#### Phase 5: Consensus
```rust
// Each general counts votes
let attack_votes = votes.iter().filter(|v| {
    matches!(&v.fields()[3], TupleField::String(s) if s == "Attack")
}).count();

// Consensus: majority (3 out of 4 = Attack)
if attack_votes >= (total_generals + 1) / 2 {
    Decision::Attack
} else {
    Decision::Retreat
}
```

### Key Coordination Patterns

| Pattern | Description | Example |
|---------|-------------|---------|
| **Broadcasting** | One writer, many readers | Commander proposes, all generals read |
| **Voting** | Many writers, many readers | Each general writes vote, all read all votes |
| **Pattern Matching** | Flexible queries | `("vote", round, _, _, _)` matches any vote in round |
| **Cross-Node Visibility** | Tuples visible globally | Node 1 writes, Node 2 reads same tuple |
| **Multi-Tenancy** | Isolated spaces | army1/consensus separate from army2/consensus |

---

## Pattern 2: Scientific Computing (Heat Diffusion)

### Use Case
Iterative solvers for partial differential equations (PDEs) on 2D grids.

### Architecture
```
┌────────────────────────────────────────────────────────────┐
│              TupleSpaceRegistry                            │
│              (Service Discovery)                           │
│                                                            │
│  Registered: ts-hpc-heat-simulation                        │
│  - tenant: "hpc-lab", namespace: "heat-diffusion"          │
│  - backend: Redis (for low-latency coordination)           │
│  - status: HEALTHY                                         │
└───────────────┬────────────────────────────────────────────┘
                │
                │ discover(tenant="hpc-lab", namespace="heat-diffusion")
    ┌───────────┴───────────┬───────────┬───────────┐
    │                       │           │           │
┌───▼────┐              ┌───▼────┐  ┌──▼─────┐  ┌──▼─────┐
│ Node 1 │              │ Node 2 │  │ Node 3 │  │ Node 4 │
│        │              │        │  │        │  │        │
│Region  │              │Region  │  │Region  │  │Region  │
│ (0,0)  │──boundary──▶ │ (0,1)  │  │ (1,0)  │  │ (1,1)  │
└───┬────┘              └───┬────┘  └───┬────┘  └───┬────┘
    │                       │           │           │
    │   boundary exchange + barrier via TupleSpace  │
    └───────────┬───────────┴───────────┴───────────┘
                ▼
     ┌──────────────────────────────────────────────┐
     │ Distributed TupleSpace (Redis)               │
     │                                              │
     │ Tuples:                                      │
     │ - ("boundary", iter, "region_0_0", "east", [temps]) │
     │ - ("boundary", iter, "region_0_1", "west", [temps]) │
     │ - ("boundary", iter, "region_1_0", "north", [temps])│
     │ - ("boundary", iter, "region_1_1", "south", [temps])│
     │ - ("barrier", "iteration_N", "region_0_0")   │
     │ - ("barrier", "iteration_N", "region_0_1")   │
     │ - ("barrier", "iteration_N", "region_1_0")   │
     │ - ("barrier", "iteration_N", "region_1_1")   │
     └──────────────────────────────────────────────┘
```

### Algorithm: Jacobi Iteration
```
For each iteration:
  1. Each region actor computes Jacobi stencil on interior cells:
     new_temp[i][j] = (temp[i-1][j] + temp[i+1][j] +
                       temp[i][j-1] + temp[i][j+1]) / 4.0

  2. All actors write their boundary values to TupleSpace

  3. All actors read neighbor boundary values from TupleSpace

  4. Barrier: Wait for all actors to complete iteration

  5. Check global convergence (max_change < threshold)

  6. Repeat until converged
```

### Coordination Flow

#### Phase 1: Region Actor Initialization
```rust
// Each region actor manages NxM cells
let actor = RegionActor::new(
    ActorPosition::new(row, col),  // Position in actor grid
    (actor_rows, actor_cols),      // Total actor grid dimensions
    (10, 10),                      // Region size (100 cells per actor)
    (100.0, 0.0, 0.0, 0.0),        // Boundary temps (top, bottom, left, right)
);
```

#### Phase 2: Compute Phase (Local, No Coordination)
```rust
// Each actor computes Jacobi iteration on its region
let max_change = actor.compute_iteration();

// Jacobi stencil (4-point average)
for i in 1..rows-1 {
    for j in 1..cols-1 {
        let avg = (temps[i-1][j] + temps[i+1][j] +
                   temps[i][j-1] + temps[i][j+1]) / 4.0;
        new_temps[i][j] = avg;
    }
}
```

**Key Point**: This is the **computation phase** - no coordination needed. Goal: maximize this to dominate coordination cost.

#### Phase 3: Boundary Exchange (Coordination via TupleSpace)

**Write Phase** (all actors write first):
```rust
// Actor (row=0, col=0) writes its boundaries
actor.write_boundaries(tuplespace).await?;

// Example: Write east boundary (right column) for neighbor (0,1) to read
let right_col: Vec<f64> = temps.iter().map(|row| row[cols-1]).collect();
let tuple = Tuple::new(vec![
    TupleField::String("boundary".to_string()),
    TupleField::Integer(iteration as i64),
    TupleField::String("region_0_0".to_string()),  // this actor
    TupleField::String("east".to_string()),        // direction
    TupleField::String(serde_json::to_string(&right_col)?),  // temps
]);
tuplespace.write(tuple).await?;
```

**Read Phase** (all actors read after all have written):
```rust
// Actor (row=0, col=1) reads west boundary from neighbor (0,0)
actor.read_boundaries(tuplespace).await?;

// Pattern: boundary from neighbor (0,0), direction "east"
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("boundary".to_string())),
    PatternField::Exact(TupleField::Integer(iteration as i64)),
    PatternField::Exact(TupleField::String("region_0_0".to_string())),
    PatternField::Exact(TupleField::String("east".to_string())),
    PatternField::Wildcard,  // any temps
]);

if let Some(tuple) = tuplespace.read(pattern).await? {
    // Update ghost cells (left column) with neighbor's boundary
    if let TupleField::String(json) = &tuple.fields()[4] {
        let values: Vec<f64> = serde_json::from_str(json)?;
        for (i, &val) in values.iter().enumerate() {
            temps[i][0] = val;  // Left column = neighbor's right column
        }
    }
}
```

#### Phase 4: Barrier Synchronization
```rust
// All actors write barrier completion tuple
let barrier_name = format!("iteration_{}", iteration);
let tuple = Tuple::new(vec![
    TupleField::String("barrier".to_string()),
    TupleField::String(barrier_name.clone()),
    TupleField::String(actor.actor_id()),
]);
tuplespace.write(tuple).await?;

// Coordinator waits for all actors to reach barrier
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("barrier".to_string())),
    PatternField::Exact(TupleField::String(barrier_name.clone())),
    PatternField::Wildcard,  // any actor
]);

while tuplespace.count(pattern.clone()).await? < total_actors {
    tokio::time::sleep(Duration::from_millis(1)).await;
}

// Clean up barrier tuples
tuplespace.take_all(pattern).await?;
```

### Key Coordination Patterns

| Pattern | Description | Example |
|---------|-------------|---------|
| **Neighbor Exchange** | Structured communication graph | Region (0,0) sends east, region (0,1) reads west |
| **Two-Phase Sync** | Avoid race conditions | All actors write, THEN all actors read |
| **Barrier Synchronization** | Global synchronization point | Wait for all actors before convergence check |
| **Pattern-Based Discovery** | Flexible neighbor lookup | `("boundary", iter, "region_0_0", "east", _)` |
| **Tuple Cleanup** | Memory management | `take_all()` removes old barrier tuples |

---

## Granularity Principle in Action

### Problem: Coordination Overhead Dominates

**Bad Design** (Too Fine-Grained):
```
❌ 100×100 grid = 10,000 actors (one per cell)
   Each iteration: 10,000 barrier waits + 40,000 boundary exchanges
   Computation per actor: 5 FLOPs (trivial)
   Communication overhead >> computation
   Result: Parallelism SLOWS DOWN the simulation
```

**Good Design** (Right Granularity):
```
✅ 100×100 grid = 100 actors (one per 10×10 region)
   Each iteration: 100 barrier waits + 400 boundary exchanges
   Computation per actor: 100 cells × 5 FLOPs = 500 FLOPs
   Communication overhead << computation (100x better ratio)
   Result: Parallelism SPEEDS UP the simulation
```

### Metrics-Driven Granularity

PlexSpaces tracks **granularity ratio**:

```rust
// Each actor tracks time spent
struct ActorMetrics {
    total_compute_ms: f64,      // Time doing Jacobi iteration
    total_coordinate_ms: f64,   // Time exchanging boundaries + barrier
    granularity_ratio: f64,     // compute / coordinate
}

// Goal: granularity_ratio >= 100.0 (compute dominates)
let ratio = total_compute_ms / total_coordinate_ms;

if ratio < target_ratio {
    warn!("Coordination overhead too high, increase region size");
} else {
    info!("✅ Granularity ratio {:.2} >= target {:.2}", ratio, target);
}
```

**Decision Matrix**:

| Grid Size | Region Size | Actors | Cells/Actor | FLOPs/Actor | Exchanges | Ratio | Result |
|-----------|-------------|--------|-------------|-------------|-----------|-------|--------|
| 100×100   | 1×1         | 10,000 | 1           | 5           | 40,000    | 0.1x  | ❌ Too slow |
| 100×100   | 5×5         | 400    | 25          | 125         | 1,600     | 5x    | ⚠️ Still slow |
| 100×100   | 10×10       | 100    | 100         | 500         | 400       | 100x  | ✅ Good |
| 100×100   | 20×20       | 25     | 400         | 2,000       | 100       | 400x  | ✅ Better |

---

## Multi-Node Distributed Coordination

### Single-Process vs Multi-Node

**Single-Process** (current heat_diffusion example):
```rust
// All actors in same process, use in-memory TupleSpace
let tuplespace = Arc::new(TupleSpace::new());
let coordinator = Coordinator::new_with_tuplespace(config, tuplespace);
```

**Multi-Node** (registry-based discovery):
```rust
// Step 1: Register TupleSpace for "hpc-lab" tenant, "heat-diffusion" namespace
let registry = Arc::new(TupleSpaceRegistry::new(kv_store));
registry.register(
    "ts-hpc-heat-simulation",
    "hpc-lab",
    "heat-diffusion",
    "localhost:9001",
    "redis",  // Redis backend for low-latency
    capabilities,
).await?;

// Step 2: Each node discovers the same TupleSpace
let tuplespace_node1 = DiscoveredTupleSpace::discover(
    registry.clone(), "hpc-lab", "heat-diffusion", shared_tuplespace.clone()
).await?;

let tuplespace_node2 = DiscoveredTupleSpace::discover(
    registry.clone(), "hpc-lab", "heat-diffusion", shared_tuplespace.clone()
).await?;

// Step 3: Each node creates its region actors
// Node 1: region (0,0), region (0,1)
// Node 2: region (1,0), region (1,1)

// Step 4: Actors coordinate via discovered TupleSpace
// Boundary exchange works across nodes automatically
```

### Cross-Node Visibility

**Key Property**: Tuples written on one node are visible on all nodes sharing the TupleSpace.

```rust
// Node 1: Region (0,0) writes east boundary
tuplespace_node1.write(Tuple::new(vec![
    TupleField::String("boundary".to_string()),
    TupleField::Integer(5),  // iteration 5
    TupleField::String("region_0_0".to_string()),
    TupleField::String("east".to_string()),
    TupleField::String(json_temps),
])).await?;

// Node 2: Region (0,1) reads west boundary
let pattern = Pattern::new(vec![
    PatternField::Exact(TupleField::String("boundary".to_string())),
    PatternField::Exact(TupleField::Integer(5)),
    PatternField::Exact(TupleField::String("region_0_0".to_string())),
    PatternField::Exact(TupleField::String("east".to_string())),
    PatternField::Wildcard,
]);

let tuple = tuplespace_node2.read(pattern).await?;
// Node 2 sees tuple written by Node 1 (cross-node visibility)
```

---

## Comparison: Byzantine vs Heat Diffusion

| Aspect | Byzantine Generals | Heat Diffusion |
|--------|-------------------|----------------|
| **Problem Domain** | Distributed consensus | Scientific computing (PDEs) |
| **Communication Pattern** | All-to-all (voting) | Neighbor-to-neighbor (boundary exchange) |
| **Data Flow** | Many-to-many | Structured grid |
| **Synchronization** | Implicit (all read all votes) | Explicit (barrier after iteration) |
| **Convergence** | Fixed rounds | Iterative (until max_change < threshold) |
| **Granularity** | 1 actor per general | 1 actor per region (100 cells) |
| **Coordination Cost** | Low (few generals) | Medium (many boundary exchanges) |
| **Computation Cost** | Low (simple voting) | High (Jacobi stencil on 100 cells) |
| **Ratio** | ~1:1 | 100:1 (target) |
| **HPC Relevance** | Fault tolerance | Performance-critical iterative solvers |

---

## Best Practices for TupleSpace Coordination

### 1. Design for Granularity
```
✅ DO: Make regions large enough that compute >> coordinate
❌ DON'T: Create one actor per data point (too fine-grained)
```

### 2. Use Two-Phase Synchronization
```rust
// ✅ CORRECT: All actors write, THEN all actors read
for actor in actors { actor.write_boundaries(tuplespace).await?; }
for actor in actors { actor.read_boundaries(tuplespace).await?; }

// ❌ WRONG: Interleaved write/read (race conditions)
for actor in actors {
    actor.write_boundaries(tuplespace).await?;
    actor.read_boundaries(tuplespace).await?; // Might miss neighbors' data
}
```

### 3. Clean Up Tuples
```rust
// ✅ DO: Remove old barrier tuples to prevent memory leak
tuplespace.take_all(barrier_pattern).await?;

// ❌ DON'T: Leave tuples accumulating in TupleSpace
```

### 4. Use Namespaces for Isolation
```rust
// ✅ DO: Use tenant + namespace for multi-tenancy
registry.register("ts-hpc-lab", "hpc-lab", "heat-diffusion", ...);
registry.register("ts-acme", "acme-corp", "production", ...);

// ❌ DON'T: Mix different simulations in same TupleSpace
```

### 5. Monitor Metrics
```rust
// ✅ DO: Track granularity ratio
let ratio = total_compute_ms / total_coordinate_ms;
if ratio < 100.0 {
    warn!("Consider increasing region size");
}

// ❌ DON'T: Blindly parallelize without measuring overhead
```

---

## Performance Characteristics

### TupleSpace Backend Selection

| Backend | Latency | Throughput | Persistence | Multi-Node | Use Case |
|---------|---------|------------|-------------|------------|----------|
| **InMemory** | <1μs | 1M ops/s | No | No | Single-process testing |
| **Redis** | ~1ms | 100K ops/s | Optional | Yes | Low-latency distributed coordination |
| **SQLite** | ~5ms | 20K ops/s | Yes | No (file-based) | Persistent single-node |
| **PostgreSQL** | ~10ms | 10K ops/s | Yes | Yes | Durable multi-node |

**Recommendation for HPC**:
- **Local development**: InMemory (fastest)
- **Distributed cluster**: Redis (low latency, high throughput)
- **Durable simulations**: PostgreSQL (persistence, recovery)

### Scaling Considerations

**Strong Scaling** (fixed problem size, more actors):
```
Problem: 1000×1000 grid (1M cells)

Setup 1: 100 actors (10K cells each)
  Compute: 10K × 5 FLOPs = 50K FLOPs per actor
  Coordinate: ~40 boundary exchanges
  Ratio: 1250:1 ✅

Setup 2: 10K actors (100 cells each)
  Compute: 100 × 5 FLOPs = 500 FLOPs per actor
  Coordinate: ~40 boundary exchanges
  Ratio: 12.5:1 ⚠️ (coordination overhead too high)
```

**Weak Scaling** (problem size grows with actors):
```
Setup 1: 1000×1000 grid, 100 actors = 10K cells/actor
Setup 2: 10000×10000 grid, 10K actors = 10K cells/actor
  Same cells/actor = same ratio (ideal weak scaling)
```

**Key Insight**: For strong scaling, **don't parallelize beyond the point where coordination dominates**.

---

## Next Steps

### For Byzantine Generals Example
See: `examples/byzantine/tests/registry_distributed_consensus.rs`
- ✅ Multi-node consensus
- ✅ Cross-node vote visibility
- ✅ Registry-based discovery
- ✅ Multi-tenancy (army1/consensus)

### For Heat Diffusion Example
See: `examples/heat_diffusion/`
- ✅ Single-process implementation complete
- 🚧 **TODO**: Multi-node distributed version with TupleSpaceRegistry
- 🚧 **TODO**: 4 nodes (one per region actor) demonstrating cross-node boundary exchange
- 🚧 **TODO**: Compare single-process vs multi-node performance

---

## Summary

PlexSpaces TupleSpace enables **elegant data parallel coordination** for HPC:

1. **Spatial Decoupling**: Producers/consumers don't need direct references
2. **Temporal Decoupling**: Writers can write before readers exist
3. **Pattern Matching**: Flexible, declarative queries
4. **Multi-Node Transparency**: Same API for local and distributed
5. **Metrics-Driven**: Track granularity to ensure performance

**Core Principle**: Keep granularity **coarse enough** that computation dominates coordination (ratio >= 100:1).

**Coordination Patterns**:
- **Broadcasting**: One-to-many (proposals, configuration)
- **Voting**: Many-to-many (consensus, reduction)
- **Neighbor Exchange**: Structured grid (boundary values)
- **Barriers**: Global synchronization (iteration lockstep)

**Result**: PlexSpaces makes distributed HPC coordination **simple, scalable, and observable**.
