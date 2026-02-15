# Real-Time Stream Processor - Clickstream Analytics

**Real-World Use Case**: Real-time clickstream analytics processing system that ingests, enriches, and aggregates user events (page views, clicks, conversions) across multiple stream processors with fault tolerance via supervision trees.

**Pattern**: Supervision trees for fault-tolerant stream processing

## Quick Start

```bash
cd examples/rust/embedded/realtime_stream_processor
cargo run
```

## What It Demonstrates

1. **Supervision Strategies** - OneForOne, OneForAll, RestForOne
2. **Failure Recovery** - Automatic restart on crashes with simulation
3. **SDK Patterns** - `#[gen_server_actor]`, `spawn_gen_server()`, `GenServerRef.cast()`/`call()`
4. **Performance Metrics** - Coordination vs computation analysis, throughput metrics
5. **Real-World Scale** - 5K events across 8 processors

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│              Clickstream Event Stream                        │
│   Events: PageView, Click, Conversion                        │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │Processor│          │Processor│          │Processor│
   │PageView │          │  Click  │          │Conversion│
   │(OneForOne)│        │(OneForOne)│        │(OneForOne)│
   └─────────┘          └─────────┘          └─────────┘
        │                     │                     │
        └─────────────────────┼─────────────────────┘
                              │
                    ┌─────────▼─────────┐
                    │   Supervisor      │
                    │  (OneForOne)      │
                    │  Auto-restart     │
                    └───────────────────┘
```

## Supervision Strategies

| Strategy | Behavior | Use Case |
|----------|----------|----------|
| **OneForOne** | Only restart the failed processor | Independent processors (page views, clicks, conversions) |
| **OneForAll** | Restart ALL processors if one fails | Shared state aggregators |
| **RestForOne** | Restart failed processor + all started after it | Ordered pipeline (Ingestion → Enrichment → Aggregation) |

## SDK Pattern

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    spawn_gen_server, GenServerRef, json,
};

// 1. Define stream processor actor with SDK annotations
#[gen_server_actor]
struct StreamProcessor {
    processor_id: String,
    event_type: EventType,
    events_processed: u64,
}

#[plexspaces_handlers(gen_server)]
impl StreamProcessor {
    // Fire-and-forget event processing
    #[handler("process_event", cast)]
    async fn handle_process_event(&mut self, _ctx: &ActorContext, msg: &Message) 
        -> Result<(), BehaviorError> {
        // Process event...
        Ok(())
    }
    
    // Request-reply metrics query
    #[handler("get_metrics")]
    async fn handle_get_metrics(&self, _ctx: &ActorContext, _msg: &Message) 
        -> Result<Value, BehaviorError> {
        // Return metrics...
        Ok(json!(metrics))
    }
}

// 2. Spawn processors using SDK helper
let processor = spawn_gen_server(
    &ctx,
    service_locator,
    "processor-0@node",
    StreamProcessor::new("processor-0", EventType::PageView),
    vec![],
).await?;

// 3. Process events (fire-and-forget)
processor.cast("process_event", &event_data).await?;

// 4. Query metrics (request-reply)
let metrics: AggregationMetrics = processor.call("get_metrics", &json!({})).await?;
```

## Failure Simulation and Restart

The example demonstrates automatic restart:

1. **Spawn processors** with supervisor
2. **Process events** (5K events)
3. **Simulate failure** in one processor
4. **Observe automatic restart** via supervisor
5. **Show recovery stats** (restarts, success/failure counts)

## Performance Metrics

The example includes comprehensive metrics:

- **Coordination vs Computation**: Measures overhead of coordination vs actual processing
- **Granularity Ratio**: Ratio of computation to coordination time
- **Efficiency**: Percentage of time spent on computation
- **Throughput**: Events per second, average latency
- **Recovery Stats**: Restart counts, success/failure rates

## Real-World Use Cases

**Use Real-Time Stream Processing when:**
- Processing high-volume event streams (clickstream, IoT, logs)
- Need fault tolerance (automatic restart on failures)
- Independent processors (OneForOne) or shared state (OneForAll)
- Ordered processing pipeline (RestForOne)

**Examples:**
- Clickstream analytics (Google Analytics, Mixpanel)
- Real-time event processing (Kafka consumers, stream processors)
- IoT data processing (sensor data, telemetry)
- Log aggregation (distributed log processing)

## See Also

- [Architecture: Supervision Trees](../../../../docs/architecture.md#supervision-trees)
- [Actor System: Supervision System](../../../../docs/actor-system.md#supervision-system)
- [SDK: Spawn Helpers](../../../../docs/sdk.md#spawn-helpers)
