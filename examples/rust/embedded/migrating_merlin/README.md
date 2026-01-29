# Merlin vs PlexSpaces Comparison

This comparison demonstrates how to implement Merlin-style HPC workflow orchestration for scientific simulation ensembles in both Merlin and PlexSpaces.

## Use Case: Ensemble Workflow Orchestration for Scientific Simulations

An HPC workflow system that:
- Orchestrates ensembles of scientific simulations
- Runs simulations in parallel with efficient enqueueing
- Aggregates results from multiple simulations
- Demonstrates HPC workflow patterns

## PlexSpaces Abstractions Showcased

- ✅ **WorkflowBehavior** - HPC workflow orchestration
- ✅ **DurabilityFacet** - Durable execution for long-running simulations
- ✅ **Ensemble Management** - Efficient orchestration of simulation ensembles

## Design Decisions

**Why WorkflowBehavior?**
- Merlin orchestrates complex HPC workflows
- WorkflowBehavior provides durable execution
- Supports long-running simulation ensembles

**Why DurabilityFacet?**
- HPC simulations can run for hours/days
- Need durability for fault tolerance
- Journaling enables replay on failure

---

## Merlin Implementation

### Native Python Code

See `native/ensemble_workflow.py` for the complete Merlin implementation.

Key features:
- **HPC Workflows**: Orchestrates scientific simulation ensembles
- **Efficient Enqueueing**: Enqueues 40M simulations in 100 seconds
- **Parallel Execution**: Runs simulations in parallel
- **Result Aggregation**: Aggregates results from multiple simulations

```python
# Usage:
workflow = Workflow("ensemble_simulation")
# Add tasks...
workflow.execute()
results = workflow.get_results()
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Ensemble workflow with durability
let behavior = Box::new(EnsembleWorkflowActor::new());
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Attach DurabilityFacet for long-running simulations
let durability_facet = Box::new(DurabilityFacet::new(storage, config));
actor.attach_facet(durability_facet, 50, serde_json::json!({})).await?;

// Start ensemble workflow
workflow.ask(StartEnsemble { ensemble_id, tasks }).await?;
```

---

## Side-by-Side Comparison

| Feature | Merlin | PlexSpaces |
|---------|--------|------------|
| **HPC Workflows** | Built-in | WorkflowBehavior |
| **Ensemble Management** | Efficient enqueueing | Parallel execution |
| **Durability** | Checkpointing | DurabilityFacet |
| **Scalability** | 40M simulations/100s | Actor-based scaling |

---

## Running the Comparison

```bash
cd examples/comparison/merlin
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Merlin HPC Workflow](https://www.hpcwire.com/off-the-wire/eflows4hpc-advances-hpc-applications-delivering-unified-solutions-for-complex-workflow-challenges-in-diverse-domains/)
- [PlexSpaces WorkflowBehavior](../../../../crates/behavior/src/workflow.rs)
