# Kueue vs PlexSpaces Comparison

This comparison demonstrates how to implement Kueue-style AI workload scheduling (Kubernetes-native ML job queueing with elastic pools) in both Kueue and PlexSpaces.

## Use Case: ML Training Job Queue with Elastic Resource Pools

An ML job queue actor that:
- Manages ML training jobs with GPU/CPU requirements
- Schedules jobs based on resource availability (elastic pools)
- Enforces resource limits (GPU memory, CPU cores)
- Demonstrates horizontal scaling and elastic worker pools
- Supports priority-based scheduling for AI workloads

## PlexSpaces Abstractions Showcased

- ✅ **GenServerBehavior** - Request-reply pattern for ML job queue operations
- ✅ **Actor Coordination** - Multiple actors coordinating via message passing
- ✅ **Resource Scheduling** - GPU/CPU-aware scheduling with elastic pools
- ✅ **Elastic Scaling** - Horizontal scaling of worker pools based on demand
- ✅ **AI Workload Management** - ML training job orchestration with resource constraints

## Design Decisions

**Why GenServerBehavior?**
- Kueue uses request-reply pattern for job operations
- Enqueue, dequeue, and status queries are synchronous operations

**Why Actor Model?**
- Kueue manages job queues as stateful services
- Actor model provides natural coordination and state management

---

## Kueue Implementation

### Native Kubernetes YAML

See `native/job_queue.yaml` for the complete Kueue configuration.

Key features:
- **Kubernetes-Native**: Job queueing for Kubernetes workloads
- **Resource Scheduling**: GPU/CPU-aware scheduling
- **Priority-Based**: Job prioritization for ML workloads
- **Elastic Pools**: Horizontal scaling of worker pools

```yaml
# Usage:
# Apply YAML to Kubernetes cluster
kubectl apply -f job_queue.yaml
# Kueue automatically schedules jobs based on resource availability
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// ML job queue actor with elastic resource pools
let behavior = Box::new(JobQueueActor::new(4, 8)); // 4 GPUs, 8 CPUs
let mut actor = ActorBuilder::new(behavior)
    .with_id(actor_id.clone())
    .build()
    .await;

// Spawn using ActorFactory
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
use std::sync::Arc;

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let actor_id = actor.id().clone();
let ctx = plexspaces_core::RequestContext::internal();
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "GenServer", // actor_type
    vec![], // initial_state
    None, // config
    std::collections::HashMap::new(), // labels
).await?;
let actor_ref = plexspaces_core::ActorRef::new(actor_id)?;

// Enqueue ML training jobs with GPU/CPU requirements
let job = MLTrainingJob {
    job_id: "resnet50-train-1".to_string(),
    model_type: "resnet50".to_string(),
    requires_gpu: true,
    gpu_memory_gb: 16, // 2 GPUs
    priority: 20,
    // ...
};
queue.ask(Enqueue { job }).await?;

// Elastic scheduling automatically allocates resources
// Workers scale horizontally based on demand
```

---

## Side-by-Side Comparison

| Feature | Kueue | PlexSpaces |
|---------|-------|------------|
| **Job Queueing** | Kubernetes CRDs | Actor-based queue |
| **Priority** | Priority field in Job | Priority in MLTrainingJob |
| **Resource Scheduling** | ClusterQueue quotas (GPU/CPU) | Elastic resource pools |
| **GPU Management** | nvidia.com/gpu resources | GPU memory tracking |
| **Elastic Scaling** | Kubernetes autoscaling | Actor pool scaling |
| **AI Workloads** | ML training jobs | MLTrainingJob with model types |

---

## Running the Comparison

```bash
cd examples/comparison/kueue
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Kueue Documentation](https://kueue.sigs.k8s.io/)
- [PlexSpaces GenServerBehavior](../../../../crates/behavior/src/genserver.rs)
