# Dirigo Reference: Distributed Stream Processing with Virtual Actors

Dirigo uses virtual actors for stream processing operators (map, filter, reduce, window). Based on [arxiv.org/abs/2308.03615](https://arxiv.org/abs/2308.03615).

## Concepts

- **Virtual actors**: Stream operators are virtual actors; activated on demand, deactivated when idle.
- **Time-sharing**: Compute resources shared across operators.
- **Operators**: Map (transform), filter (threshold), reduce (aggregate), window (windowed aggregation).

## Native Python-style usage

```python
from dirigo import VirtualActor, StreamOperator

map_op = VirtualActor.create(operator_type="map", config={"function": "transform"})
filter_op = VirtualActor.create(operator_type="filter", config={"threshold": 50.0})
reduce_op = VirtualActor.create(operator_type="reduce", config={"window_size": 5})

for event in sensor_events:
    transformed = map_op.process(event)
    if transformed.value > 50.0:
        filtered = filter_op.process(transformed)
        aggregated = reduce_op.process(filtered)
```

## PlexSpaces mapping

| Dirigo | PlexSpaces |
|--------|------------|
| Virtual actor per operator | WorkflowActor + virtual_actor facet (one actor type, instance per stream/run) |
| Operator process(event) | run(payload) with events[] batch; or onIngest (single event) |
| Window aggregation | State: window[], window_size; emit when full in run() or onWindow_flush |
| Map/filter/reduce chain | Single actor with windowed aggregation; chain can be multiple actors (future) |

This example implements **windowed aggregation** in one actor: run() processes event batches and emits aggregates when the window is full; ingest and window_flush handlers support single-event and explicit flush.
