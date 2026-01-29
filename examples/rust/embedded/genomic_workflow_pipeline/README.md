# Genomic Workflow Pipeline Example

## Overview

A simplified example demonstrating PlexSpaces **workflow orchestration** with a genomic analysis pipeline. This example showcases:

- **Workflow Definition**: Multi-step pipeline with typed configurations
- **Workflow Execution**: Real processing with simulated genomic operations
- **ConfigBootstrap**: Erlang/OTP-style configuration loading
- **CoordinationComputeTracker**: Standardized metrics for tracking coordination vs compute
- **Durability**: Workflow state persistence via journaling (via WorkflowStorage)
- **🔄 Automated Recovery**: Auto-recovery of interrupted workflows on node startup
- **💾 Multi-Database Support**: SQLite (testing/embedded) and PostgreSQL (production)

**Key Difference from genomics-pipeline**: This example uses the `plexspaces-workflow` crate to define and execute workflows, while the genomics-pipeline example uses actors directly with supervision trees.

## What This Demonstrates

### PlexSpaces Workflow Features
- ✅ **Workflow Definition**: Multi-step pipeline with typed configurations
- ✅ **Workflow Execution**: Real processing with simulated genomic operations
- ✅ **Workflow Storage**: In-memory storage for definitions and execution metadata
- ✅ **Metrics Tracking**: Compute vs coordinate overhead measurement

### CLAUDE.md Principle #6: Granularity vs Communication Cost
- ✅ **Metrics Tracking**: Compute vs coordinate overhead measurement
- ✅ **Granularity Ratio**: `compute_time / coordinate_time` calculation
- ✅ **Performance Evaluation**: Compares against 100× target ratio
- ✅ **Efficiency Metrics**: Shows overhead percentage

### Pipeline Steps
1. **Quality Control (QC)**: Filter low-quality sequence reads
2. **Genome Alignment**: Map reads to reference genome (hg38)
3. **Variant Calling**: Identify genetic variations

## Architecture

### Standalone Runner
The example uses a standalone runner (`main.rs`) that:
- Accepts number of reads as CLI argument
- Loads configuration via `ConfigBootstrap`
- Creates workflow definition with 3 steps
- Executes pipeline steps directly (simplified for demonstration)
- Tracks metrics using `CoordinationComputeTracker`
- Saves results to `pipeline_results.json`

### Actors (Optional)
The example includes actor implementations (`ActorBehavior`) that can be used for node-based deployment:
- `QcActorBehavior`: Quality control processing
- `AlignmentActorBehavior`: Genome alignment
- `VariantCallingActorBehavior`: Variant calling

These actors are available for integration with `NodeBuilder`/`ActorBuilder` patterns in production deployments.

## Running the Example

### Quick Start

```bash
# From example directory
cd examples/genomic-workflow-pipeline

# Run with default 1000 reads
./scripts/run.sh

# Or specify number of reads
./scripts/run.sh 5000

# Or use cargo directly
cargo run --release -- 1000
```

### Workflow Recovery & Auto-Recovery

The example showcases **robust workflow recovery** with automated recovery on startup. See [RECOVERY_BEST_PRACTICES.md](RECOVERY_BEST_PRACTICES.md) for detailed documentation.

#### Manual Recovery

```bash
# Start a workflow (saves execution ID to last_execution_id.txt)
cargo run --release -- 1000

# If interrupted, resume using the execution ID
cargo run --release -- --resume <execution_id>

# Or resume using the saved execution ID
EXEC_ID=$(cat last_execution_id.txt)
cargo run --release -- --resume $EXEC_ID
```

#### Automated Recovery on Startup

```bash
# Enable auto-recovery to automatically resume interrupted workflows
cargo run --release -- --auto-recover 1000

# Auto-recovery will:
# 1. Find all RUNNING/PENDING workflows
# 2. Claim ownership of workflows owned by this node (handles node-id changes)
# 3. Transfer ownership of stale workflows from dead nodes
# 4. Resume all claimed workflows automatically
```

#### Database Support

The example supports both SQLite and PostgreSQL:

```bash
# SQLite (default, for testing/embedded)
cargo run --release -- --storage workflow.db 1000

# PostgreSQL (for production multi-node deployments)
cargo run --release -- --storage "postgresql://user:pass@localhost/workflow_db" 1000
```

**How Recovery Works:**
1. **Persistent Storage**: Workflow state is saved to database (SQLite or PostgreSQL)
   - SQLite: File-based storage (default: `workflow.db`)
   - PostgreSQL: Connection string for multi-node deployments
   - Falls back to in-memory storage if database creation fails
2. **Execution State**: Each workflow execution has:
   - Unique execution ID
   - Status (PENDING, RUNNING, COMPLETED, FAILED)
   - Version (for optimistic locking)
   - Node ownership (for multi-node recovery)
   - Heartbeat (for health monitoring)
3. **Resume Capability**: Use `--resume <execution_id>` to continue from the last checkpoint
   - Checks execution status and resumes if in RUNNING/PENDING state
   - Uses `WorkflowExecutor::execute_from_state()` to continue execution
4. **Auto-Recovery**: Use `--auto-recover` to automatically resume interrupted workflows on startup
   - Finds all RUNNING/PENDING workflows
   - Claims ownership with optimistic locking (prevents race conditions)
   - Transfers stale workflows from dead nodes (based on heartbeat threshold)
   - Resumes all claimed workflows automatically

**Recovery Scenarios:**
- **Node Crash**: Workflow state persists in database, resume with `--resume <execution_id>`
- **Interrupted Execution**: Execution ID saved to `last_execution_id.txt` for easy recovery
- **Failed Workflow**: Failed workflows can be restarted (new execution) or investigated
- **Completed Workflow**: Resuming a completed workflow shows completion status

**Recovery Implementation:**
- Uses `WorkflowStorage::new_file()` or `WorkflowStorage::new_postgres()` for persistent storage
- `WorkflowExecutor::execute_from_state()` resumes from last checkpoint
- Execution state tracked in `workflow_executions` table with version and heartbeat
- Step execution history preserved for debugging
- Optimistic locking prevents race conditions in multi-node deployments

### Recovery Best Practices

This implementation follows best practices from **Temporal** and **AWS Step Functions**:

#### Key Principles

1. **Durable Execution**: Workflows are "crash-proof" - state is persisted before each step
2. **Auto-Recovery on Startup**: Node/app queries for RUNNING/PENDING workflows on startup and automatically resumes them
3. **Optimistic Locking**: Version-based concurrency control prevents race conditions
4. **Node Ownership**: Each workflow owned by exactly one node at a time
5. **Health Monitoring**: Heartbeat-based detection of stale/dead nodes

#### Recovery Flow

1. **Find Interrupted Workflows**: Query for RUNNING/PENDING workflows (owned by any node or unowned)
2. **Claim Ownership**: Update `node_id` with optimistic locking (prevents race conditions)
3. **Recover Workflow**: Resume from last checkpoint using `WorkflowExecutor::execute_from_state()`

#### Recovery Scenarios

- **Normal Node Restart (Same node-id)**: Finds workflows owned by this node-id, updates heartbeat, recovers
- **Node-ID Changed**: Finds workflows with old node-id, claims ownership by updating node_id, recovers
- **Dead Node Recovery**: Finds stale workflows (no heartbeat in X minutes), transfers ownership, recovers
- **Concurrent Recovery (Multi-Node)**: Optimistic locking ensures only one node succeeds in claiming ownership

#### Testing Recovery

**Manual Testing:**
```bash
# 1. Start workflow
cargo run --release -- 1000

# 2. Kill process (simulate crash)
kill -9 <pid>

# 3. Resume workflow
EXEC_ID=$(cat last_execution_id.txt)
cargo run --release -- --resume $EXEC_ID
```

**Automated Testing:**
```bash
# Run recovery validation script
./scripts/validate_recovery.sh

# Test auto-recovery
./scripts/test_auto_recovery.sh
```

**Database Inspection:**
```bash
# List all executions (if sqlite3 available)
sqlite3 workflow.db "SELECT execution_id, status, created_at FROM workflow_executions ORDER BY created_at DESC;"

# Find RUNNING workflows (for recovery)
sqlite3 workflow.db "SELECT execution_id, status, updated_at FROM workflow_executions WHERE status = 'RUNNING';"
```

### Configuration

Configuration is loaded via `ConfigBootstrap` from:
1. Environment variables (highest precedence)
2. `release.toml` file
3. Default values (lowest precedence)

Example `release.toml`:
```toml
[genomic_pipeline]
target_granularity_ratio = 100.0

[genomic_pipeline.node]
id = "genomic-pipeline-node"
listen_address = "0.0.0.0:9000"
```

### Expected Output

```
╔════════════════════════════════════════════════════════════╗
║    PlexSpaces Genomic Workflow Pipeline (With Metrics)    ║
╚════════════════════════════════════════════════════════════╝

📋 Configuration:
   Total reads: 1000
   Target ratio: 100× (compute/coordinate)

✓ Workflow definition created with 3 steps

✓ Generating 1000 test reads...
✓ Running QC...
✓ Running Alignment...
✓ Running Variant Calling...

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                    PIPELINE RESULTS
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Throughput:
  Total reads:        1000
  QC passed:          950 (95.0%)
  Variants called:    850

Performance Metrics (CLAUDE.md Principle #6):
  ┌─────────────────────┬──────────┐
  │ Metric              │ Value    │
  ├─────────────────────┼──────────┤
  │ Compute Time        │      80ms │
  │ Coordinate Time     │       3ms │
  │ Total Time          │      83ms │
  ├─────────────────────┼──────────┤
  │ Granularity Ratio   │    26.7× │
  │ Efficiency          │   96.4% │
  └─────────────────────┴──────────┘

  Status:             ⚠  ACCEPTABLE
  Throughput:         11,743 reads/second

✓ Results saved to pipeline_results.json
```

## Code Structure

```
genomic-workflow-pipeline/
├── src/
│   ├── main.rs              # Standalone workflow runner
│   ├── config.rs            # Configuration using ConfigBootstrap
│   ├── types.rs             # Data structures (SequenceRead, QCResult, etc.)
│   ├── processor.rs         # Simulated genomic operations
│   ├── actors/              # Actor behaviors (optional, for node deployment)
│   │   ├── qc_actor.rs
│   │   ├── alignment_actor.rs
│   │   └── variant_calling_actor.rs
│   └── application.rs       # Application trait (for node deployment)
├── scripts/
│   ├── run.sh               # Run script
│   ├── test_recovery.sh      # Basic recovery test
│   ├── validate_recovery.sh  # Comprehensive recovery validation
│   └── test_auto_recovery.sh # Auto-recovery test
├── release.toml             # Configuration file
└── README.md                # This file
```

## Framework Abstractions Used

This example demonstrates the use of:

1. **ConfigBootstrap**: Erlang/OTP-style configuration loading
   ```rust
   let config: GenomicPipelineConfig = ConfigBootstrap::load().unwrap_or_default();
   ```

2. **CoordinationComputeTracker**: Standardized metrics tracking
   ```rust
   let mut metrics_tracker = CoordinationComputeTracker::new("genomic-pipeline".to_string());
   metrics_tracker.start_coordinate();
   // ... do work ...
   metrics_tracker.end_coordinate();
   let metrics = metrics_tracker.finalize();
   ```

3. **WorkflowStorage**: Workflow definition and execution metadata storage
   ```rust
   let storage = WorkflowStorage::new_in_memory().await?;
   storage.save_definition(&definition).await?;
   ```

5. **ActorBehavior**: Actor implementations for node-based deployment
   ```rust
   impl ActorBehavior for QcActorBehavior {
       async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
           // Process message
       }
   }
   ```

## Testing

```bash
# Run unit tests
cargo test

# Run with different read counts
./scripts/run.sh 1000
./scripts/run.sh 5000
./scripts/run.sh 10000

# Test recovery functionality
./scripts/test_recovery.sh
./scripts/validate_recovery.sh
./scripts/test_auto_recovery.sh
```

## Notes

- **Workflow Execution**: Uses `WorkflowExecutor` to execute workflow definitions with proper state management
- **Durability & Recovery**: 
  - Workflow state persisted to SQLite database (`workflow.db` by default)
  - Falls back to in-memory storage if file creation fails
  - Resume interrupted workflows with `--resume <execution_id>`
  - Execution IDs saved to `last_execution_id.txt` for easy recovery
- **Metrics**: Tracks coordination vs compute metrics to demonstrate CLAUDE.md Principle #6 (Granularity vs Communication Cost)
- **Actors**: Actor implementations (`ActorBehavior`) provided for node-based deployment but not used in standalone runner
- **Storage**: Uses `WorkflowStorage::new_file()` for persistent storage; `WorkflowExecutor::execute_from_state()` for recovery

## Related Examples

- `genomics-pipeline`: Similar example using actors directly with supervision trees
- `order-processing`: Complex workflow orchestration example
- `finance-risk`: Worker pool pattern with workflow coordination
