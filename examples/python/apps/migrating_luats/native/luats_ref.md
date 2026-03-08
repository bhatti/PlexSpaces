# LuaTS Reference: Linda + Event-Driven Programming

LuaTS combines Linda (tuple spaces) with event-driven programming to simplify multi-thread coordination.

## Concepts

- **Linda**: Associative shared memory; processes communicate via tuples and pattern matching (`in`, `out`, `rd`).
- **Events**: Subscribe/publish; reactive notification when data or conditions change.
- **LuaTS**: Unified API that uses both—e.g. publish an event and also write a tuple so other processes can `rd`/`in` by pattern.

## Native LuaTS Usage

```lua
local luats = require("luats")

-- Subscribe to events (event-driven)
luats.subscribe("data_ready", function(event)
    print("Received event:", event.payload)
end)

-- Publish event (also writes to TupleSpace)
luats.publish("data_ready", {
    payload = "processed_data",
    timestamp = os.time()
})

-- Read from TupleSpace (Linda pattern)
local tuple = luats.read({"event", "data_ready", _})

-- Write to TupleSpace
luats.write({"event", "data_ready", "data"})
```

## PlexSpaces Mapping

| LuaTS | PlexSpaces |
|-------|------------|
| `luats.write(tuple)` | `host.ts.write([prefix, id, tag, ...])` |
| `luats.read(pattern)` | `host.ts.read(pattern)` / `host.ts.take(pattern)` |
| `luats.publish(type, data)` | Write tuple + optional process group broadcast |
| `luats.subscribe(type, fn)` | Process group join + handle cast messages |
| Event loop / threads | Workflow run + handlers; timer via `host.send_after` |

This example uses TupleSpace as the event buffer (CDC-style): events are tuples `(prefix, pipeline_id, "event", source, seq, payload)`. Consumers take/read by pattern. Window flush (timer-style) is a handler that takes all matching tuples and aggregates.
