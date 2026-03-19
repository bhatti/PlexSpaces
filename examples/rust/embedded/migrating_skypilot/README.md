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

## PlexSpaces implementation

The runnable example is in `src/main.rs`: it builds a node, registers a GenServer-style scheduler actor, and demonstrates task submission. For **new** code, prefer the patterns in [Architecture](../../../../docs/architecture.md) and [SDK](../../../../docs/sdk.md): SDK annotations (`#[gen_server_actor]`, `#[handler("op")]`), `plexspaces_sdk` spawn helpers, `call_message` / `cast_message` (or `new_message`), and `RequestContext::new_without_auth(tenant, namespace)` (not `internal()`). Avoid reaching for `ActorFactory` in application examples unless you are intentionally showing the low-level path.

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
