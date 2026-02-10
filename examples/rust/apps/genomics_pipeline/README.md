# Genomics Pipeline

**Real-World Use Case**: DNA sequencing analysis workflow inspired by GATK/Illumina bioinformatics pipelines.

## Quick Start

```bash
cd examples/rust/apps/genomics_pipeline
cargo run
```

## What It Demonstrates

1. **Workflow Actor** - Durable multi-step pipeline execution
2. **Run Handler** - Main pipeline (QC → Alignment → Variant Calling)
3. **Signal Handlers** - Pause, resume, cancel operations
4. **Query Handlers** - Status, progress, performance metrics
5. **Performance Tracking** - Compute vs coordinate time (granularity ratio)

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                 GenomicsPipelineWorkflow                        │
├─────────────────────────────────────────────────────────────────┤
│  State: PipelineState                                           │
│    - sample_id, status, current_step                           │
│    - qc_result, alignment_result, variant_result               │
│    - compute_time_ms, coordinate_time_ms                       │
│                                                                 │
│  Handlers:                                                      │
│    #[run_handler]              → Run 3-step pipeline            │
│    #[signal_handler("pause")]  → Pause execution                │
│    #[signal_handler("resume")] → Resume execution               │
│    #[signal_handler("cancel")] → Cancel with reason             │
│    #[query_handler("status")]  → Get current status             │
│    #[query_handler("progress")]→ Get detailed progress          │
│    #[query_handler("metrics")] → Get performance metrics        │
└─────────────────────────────────────────────────────────────────┘
```

## Pipeline Steps

```
Input: FASTQ reads
       │
       ▼
┌──────────────┐
│ 1. QC        │ Filter low-quality reads, check adapters
│              │ Output: passed_reads, avg_quality, gc_content
└──────────────┘
       │
       ▼
┌──────────────┐
│ 2. Alignment │ Map reads to reference genome (hg38)
│              │ Output: aligned_reads, alignment_rate
└──────────────┘
       │
       ▼
┌──────────────┐
│ 3. Variants  │ Call SNPs and indels
│              │ Output: total_variants, snps, indels
└──────────────┘
       │
       ▼
Output: PipelineState with all results
```

## SDK Pattern

```rust
use plexspaces_sdk::*;

// 1. Define workflow actor
#[workflow_actor]
struct GenomicsPipelineWorkflow {
    state: PipelineState,
    input: Option<PipelineInput>,
}

// 2. Add workflow handlers
#[plexspaces_handlers(workflow)]
impl GenomicsPipelineWorkflow {
    // Main pipeline execution
    #[run_handler]
    async fn run(&mut self, ctx: &ActorContext, input: Message) 
        -> Result<Message, BehaviorError> {
        // Step 1: Quality Control
        // Step 2: Alignment
        // Step 3: Variant Calling
    }
    
    // Pause signal
    #[signal_handler("pause")]
    async fn on_pause(&mut self, ctx: &ActorContext, data: Message) 
        -> Result<(), BehaviorError> {
        self.state.is_paused = true;
        Ok(())
    }
    
    // Query metrics
    #[query_handler("metrics")]
    async fn get_metrics(&self, ctx: &ActorContext, params: Message) 
        -> Result<Message, BehaviorError> {
        // Return compute/coordinate time ratio
    }
}

// 3. Spawn and run workflow
let workflow: WorkflowRef = spawn_workflow_actor(
    &ctx, service_locator, "sample-001", "sequencing",
    GenomicsPipelineWorkflow::new(), vec![],
).await?;

let result: PipelineState = workflow.run(&pipeline_input).await?;
let metrics: Value = workflow.query("metrics").await?;
```

## Key APIs

| API | Purpose |
|-----|---------|
| `#[workflow_actor]` | Mark struct as Workflow actor |
| `#[run_handler]` | Main workflow execution entry |
| `#[signal_handler("name")]` | Handle external signals (pause/resume/cancel) |
| `#[query_handler("name")]` | Handle read-only queries (status/progress/metrics) |
| `spawn_workflow_actor()` | Spawn workflow with SDK helper |
| `WorkflowRef::run()` | Execute workflow with typed I/O |
| `WorkflowRef::signal()` | Send signal to workflow |
| `WorkflowRef::query()` | Query workflow state |

## Performance Metrics

The example tracks **granularity ratio** (compute time / coordinate time):

- **< 10×**: Too much coordination overhead
- **10×-100×**: Acceptable for small workloads
- **> 100×**: Excellent efficiency

## Use Cases

- DNA sequencing (Illumina, PacBio, Nanopore)
- Whole genome sequencing (WGS)
- Exome sequencing
- Clinical diagnostics
- Cancer genomics
- Variant annotation pipelines

## See Also

- [SDK Documentation](../../../../docs/sdk.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
