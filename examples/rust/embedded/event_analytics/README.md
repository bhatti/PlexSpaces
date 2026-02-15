# Event Analytics - Distributed Event Tracking

**Real-World Use Case**: Web analytics/event tracking system (Google Analytics, Mixpanel-style) that tracks page views, clicks, and conversions across multiple shards for horizontal scaling.

**Pattern**: Shard Groups (data-parallel horizontal scaling via hash-based sharding)

## Quick Start

```bash
cd examples/rust/embedded/event_analytics
cargo run
```

## What It Demonstrates

1. **Event Tracking** - Track page views, clicks, and conversions (Google Analytics-style)
2. **Shard Groups** - Data-parallel horizontal scaling via hash-based sharding
3. **Hash-Based Routing** - Partition key (user_id) → hash → shard_id → specific actor
4. **Scatter-Gather Queries** - Query all shards and aggregate results
5. **SDK Patterns** - `#[gen_server_actor]`, `spawn_gen_server()`, `call_message()`, `cast_message()`
6. **Performance Metrics** - Coordination vs computation analysis, throughput metrics

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Analytics Client                         │
│   Track Event: user_id="user-123"                          │
└─────────────────────────────────────────────────────────────┘
                              │
                    Hash(user_id) → shard_id=5
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │ Shard 0 │          │ Shard 5 │          │ Shard 7 │  ...
   │ Events  │          │ Events  │          │ Events  │
   │ 1,234   │          │ 1,567   │          │ 1,890   │
   └─────────┘          └─────────┘          └─────────┘
        │                     │                     │
        └─────────────────────┼─────────────────────┘
                              │
                    Scatter-Gather Query
                              │
                              ▼
                    Aggregate Totals
```

## SDK Pattern

```rust
use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    spawn_gen_server, cast_message, call_message, json,
};

// 1. Define shard actor with SDK annotations
#[gen_server_actor]
struct AnalyticsShard {
    shard_id: usize,
    page_views: HashMap<String, u64>,
    clicks: HashMap<String, u64>,
    conversions: HashMap<String, u64>,
}

#[plexspaces_handlers(gen_server)]
impl AnalyticsShard {
    // Fire-and-forget event tracking
    #[handler("track_event", cast)]
    async fn handle_track_event(&mut self, _ctx: &ActorContext, msg: &Message) 
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

// 2. Spawn shard actors using SDK helper
let shard = spawn_gen_server(
    &ctx,
    service_locator,
    "analytics-shard-0@node",
    AnalyticsShard::new(0),
    vec![],
).await?;

// 3. Track events (fire-and-forget)
let event_msg = cast_message(json!({
    "user_id": "user-123",
    "page_id": "page-456",
    "event_type": "PageView",
}));
shard.tell(event_msg).await?;

// 4. Query metrics (request-reply)
let query_msg = call_message(json!({}));
let reply = shard.ask(query_msg, Duration::from_secs(5)).await?;
let metrics: ShardMetrics = serde_json::from_slice(&reply.payload)?;
```

## Hash-Based Routing

```rust
fn route_to_shard(key: &str, shard_count: usize) -> usize {
    let hash = key.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % shard_count as u64) as usize
}

// Route event to shard based on user_id
let shard_id = route_to_shard(&event.user_id, shard_count);
shards[shard_id].tell(event_msg).await?;
```

## Scatter-Gather Pattern

```rust
// Query all shards in parallel
let mut futures = Vec::new();
for shard in &shards {
    let query_msg = call_message(json!({}));
    futures.push(shard.ask(query_msg, Duration::from_secs(5)));
}

// Collect results
let results: Vec<Result<Message, _>> = futures::future::join_all(futures).await;

// Aggregate totals
let mut total_page_views = 0u64;
for result in results {
    let metrics: ShardMetrics = serde_json::from_slice(&result?.payload)?;
    total_page_views += metrics.page_views;
}
```

## Performance Metrics

The example demonstrates:
- **Coordination vs Computation** - Tracks coordination overhead vs actual event processing
- **Granularity Ratio** - Compute time / coordinate time (should be >= 10x)
- **Throughput** - Events per second
- **Shard Distribution** - Even distribution across shards (hash-based)

## Real-World Use Cases

This example demonstrates patterns used by:
- **Google Analytics** - Page view tracking across distributed servers
- **Mixpanel** - Event tracking with horizontal scaling
- **Segment** - Customer data platform with event aggregation
- **Amplitude** - Product analytics with distributed counters

**Use Shard Groups when:**
- High-throughput workloads (millions of events/sec)
- Horizontally scalable state (counters, metrics, analytics)
- Partitionable data (user IDs, session IDs, page IDs)
- Eventual consistency is acceptable
- Need scatter-gather queries

**Other Use Cases:**
- Distributed counters/metrics
- Time-series data aggregation
- Session management
- Rate limiting
- Leaderboards
- Inventory tracking

**Don't use when:**
- Need broadcast/coordination → Use Process Groups (Pub/Sub)
- Strong consistency required → Use single actor
- Small datasets → Overhead not worth it

## See Also

- [Process Groups (Pub/Sub)](../chat_room/) - For broadcast patterns
- [Architecture Docs](../../../../docs/architecture.md)
- [SDK Documentation](../../../../docs/sdk.md)
- [Getting Started](../../../../docs/getting-started.md)
