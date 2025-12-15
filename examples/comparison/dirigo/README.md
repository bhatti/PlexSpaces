# Dirigo vs PlexSpaces Comparison

This comparison demonstrates how to implement Dirigo-style distributed stream processing with virtual actors for real-time event processing in both Dirigo and PlexSpaces.

## Use Case: Real-time Event Stream Processing

A stream processing system that:
- Processes sensor events in real-time
- Transforms events using map operators
- Filters events by threshold using filter operators
- Aggregates values using reduce operators
- Demonstrates virtual actors for stream operators with time-sharing compute resources

**Stream Pipeline**: Sensor Events → Map (Transform) → Filter (Threshold) → Reduce (Aggregate)

---

## Dirigo Implementation

### Native Python Code

See `native/stream_processing.py` for the complete Dirigo implementation.

Key features:
- **Virtual Actors**: Stream operators as virtual actors
- **Time-Sharing**: Compute resources shared efficiently
- **Performance Isolation**: Operators isolated from each other
- **Message-Level Scheduling**: Fine-grained scheduling

The example demonstrates:
1. Map operator (transform sensor data)
2. Filter operator (filter by threshold)
3. Reduce operator (aggregate in windows)

---

## PlexSpaces Implementation

### Rust Code

```rust
// Stream processing with virtual actors (Dirigo pattern)
// Create operators (virtual actors)
let map_operator = StreamOperator {
    operator_id: "map-1".to_string(),
    operator_type: "map".to_string(),
    config: HashMap::new(),
};

let filter_operator = StreamOperator {
    operator_id: "filter-1".to_string(),
    operator_type: "filter".to_string(),
    config: {
        let mut config = HashMap::new();
        config.insert("threshold".to_string(), json!(50.0));
        config
    },
};

let reduce_operator = StreamOperator {
    operator_id: "reduce-1".to_string(),
    operator_type: "reduce".to_string(),
    config: {
        let mut config = HashMap::new();
        config.insert("window_size".to_string(), json!(5));
        config
    },
};

// Create actors with VirtualActorFacet
let map_actor = node.spawn_actor(create_operator_actor(map_operator)).await?;
let filter_actor = node.spawn_actor(create_operator_actor(filter_operator)).await?;
let reduce_actor = node.spawn_actor(create_operator_actor(reduce_operator)).await?;

// Process events in pipeline
for event in sensor_events {
    // Map: Transform
    let transformed = map_actor.ask(ProcessEvent { event }).await?;
    
    // Filter: Filter by threshold
    if transformed.value > 50.0 {
        let filtered = filter_actor.ask(ProcessEvent { event: transformed }).await?;
        
        // Reduce: Aggregate
        let aggregated = reduce_actor.ask(ProcessEventBatch { events: vec![filtered] }).await?;
    }
}
```

---

## Side-by-Side Comparison

| Feature | Dirigo | PlexSpaces |
|---------|--------|------------|
| **Virtual Actors** | Built-in | VirtualActorFacet |
| **Stream Operators** | Virtual actors | GenServerBehavior |
| **Time-Sharing** | Built-in | DurabilityFacet |
| **Serverless** | Architecture | Actor-based |
| **Performance Isolation** | Built-in | Actor isolation |

---

## Key Features

### Virtual Actors for Stream Operators
- **Automatic Activation**: Operators activated on first event
- **Automatic Deactivation**: Operators deactivated after idle timeout
- **Model Caching**: Operators cache state for efficient processing
- **Time-Sharing**: Compute resources shared efficiently

### Stream Processing Pipeline
- **Map**: Transform events (e.g., format conversion)
- **Filter**: Filter events by condition (e.g., threshold)
- **Reduce**: Aggregate values in window (e.g., average)
- **Window**: Windowed aggregation (e.g., max/min in window)

### Performance Isolation
- **Resource Efficiency**: High resource utilization
- **Performance Isolation**: Operators isolated from each other
- **Dynamic Scaling**: Operators scale based on load
- **Message-Level Scheduling**: Fine-grained scheduling

---

## Running the Comparison

```bash
cd examples/comparison/dirigo
cargo build --release
cargo run --release
./scripts/test.sh
```

---

## References

- [Dirigo Paper](https://arxiv.org/abs/2308.03615)
- [PlexSpaces GenServerBehavior](../../../../crates/behavior/src/genserver.rs)
- [PlexSpaces VirtualActorFacet](../../../../crates/journaling/src/virtual_actor.rs)
