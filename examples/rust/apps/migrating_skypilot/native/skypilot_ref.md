# SkyPilot native reference: multi-cloud ML orchestration

This file describes how the same use case is implemented with [SkyPilot](https://github.com/skypilot-org/skypilot).

## SkyPilot model

- **Multi-cloud**: Schedule jobs across AWS, GCP, Azure; pick cheapest instance that meets requirements.
- **Resource matching**: Specify accelerators (e.g. V100:1), CPUs, memory; SkyPilot finds matching instances.
- **Spot instances**: Optional `use_spot=True` for lower cost with possible preemption.
- **Cost optimization**: SkyPilot compares prices across clouds and selects the best option.

## Native pattern (conceptual)

```python
import sky

@sky.task
def train_model():
    # ML training code
    pass

# Run on cheapest GPU across clouds
job = sky.launch(
    train_model,
    resources=sky.Resources(accelerators="V100:1"),
)

# With cloud preference
job = sky.launch(
    train_model,
    resources=sky.Resources(accelerators="V100:1", cloud="aws"),
)
```

## PlexSpaces equivalent (Rust WASM app)

- **GenServer actor**: Single scheduler actor (virtual_actor + durability); ops: `submit_task`, `get_best_resources`, `get_status`.
- **Cloud catalog**: In-memory catalog of instance types (AWS, GCP) with cost and availability; `find_best_resources` picks cheapest match.
- **State**: Task queue and running tasks stored in actor state; durability facet checkpoints for recovery.

See README comparison table (SkyPilot vs PlexSpaces).
