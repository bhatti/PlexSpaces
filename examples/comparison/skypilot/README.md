# SkyPilot vs PlexSpaces Comparison

This comparison demonstrates how to implement SkyPilot-style multi-cloud AI workload orchestration with cost optimization in both SkyPilot and PlexSpaces.

## Use Case: Multi-Cloud Resource Scheduling for AI Workloads

A resource scheduler that:
- Finds cheapest available resources across AWS, GCP, Azure
- Matches task requirements (GPU, CPU, memory) to instances
- Optimizes costs while meeting performance requirements
- Demonstrates multi-cloud orchestration

## PlexSpaces Abstractions Showcased

- ✅ **GenServerBehavior** - Request-reply pattern for task scheduling
- ✅ **Actor Coordination** - Resource scheduling across multiple clouds
- ✅ **Resource-Aware Scheduling** - Matches requirements to instances
- ✅ **Cost Optimization** - Finds cheapest available resources

## Design Decisions

**Why Actor Model for Resource Scheduling?**
- SkyPilot manages resource allocation as stateful service
- Actor model provides natural coordination and state management
- Can scale horizontally for large cloud catalogs

**Why Multi-Cloud Support?**
- SkyPilot finds cheapest resources across providers
- Reduces vendor lock-in
- Improves availability (fallback to other clouds)

---

## SkyPilot Implementation

### Native Python Code

See `native/ai_workload.py` for the complete SkyPilot implementation.

Key features:
- **Multi-Cloud**: Finds cheapest resources across AWS, GCP, Azure
- **Cost Optimization**: Automatic cost comparison and selection
- **Resource Matching**: Matches task requirements to instances
- **Spot Instances**: Supports spot instances for cost savings

```python
# Usage:
job = sky.launch(
    train_model,
    resources=sky.Resources(accelerators="V100:1", use_spot=True),
)
```

---

## PlexSpaces Implementation

### Rust Code

```rust
// Multi-cloud resource scheduler
use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
use plexspaces_mailbox::{mailbox_config_default, Mailbox};
use std::sync::Arc;

let behavior = Box::new(SkyPilotSchedulerActor::new());
let actor_id = "scheduler@node1".to_string();
let mut mailbox_config = mailbox_config_default();
mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id)).await?;
let actor = Actor::new(actor_id.clone(), behavior, mailbox, "default".to_string(), None);

let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
    .ok_or_else(|| "ActorFactory not found")?;
let ctx = plexspaces_core::RequestContext::internal();
let _message_sender = actor_factory.spawn_actor(
    &ctx,
    &actor_id,
    "GenServer", // actor_type
    vec![], // initial_state
    None, // config
    std::collections::HashMap::new(), // labels
).await?;
let scheduler = plexspaces_core::ActorRef::new(actor_id)?;

// Submit AI task (SkyPilot finds cheapest resources)
let task = AITask {
    task_id: "training-1".to_string(),
    gpu_required: true,
    gpu_memory_gb: 16,
    cpu_cores: 4,
    memory_gb: 16,
    cloud_preference: None, // Find cheapest
};

let allocation = scheduler.ask(SubmitTask { task }).await?;
// Returns: cheapest available instance across AWS, GCP, Azure
```

---

## Side-by-Side Comparison

| Feature | SkyPilot | PlexSpaces |
|---------|----------|------------|
| **Multi-Cloud** | AWS, GCP, Azure | Actor-based scheduling |
| **Cost Optimization** | Finds cheapest | Cost-aware algorithm |
| **Resource Matching** | Automatic | Requirement matching |
| **AI Workloads** | ML training/inference | AITask with GPU/CPU reqs |

---

## Running the Comparison

```bash
cd examples/comparison/skypilot
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [SkyPilot Documentation](https://skypilot.readthedocs.io/)
- [PlexSpaces Actor Coordination](../../../../crates/core/src/actor_context.rs)
