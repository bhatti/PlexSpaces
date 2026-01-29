# Actor Groups (Sharding) Example

**Purpose**: Demonstrate data-parallel horizontal scaling via sharding.

**Pattern**: Partition key → hash → shard_id → specific actor

## Quick Start

```bash
cd examples/rust/embedded/actor_groups_sharding

# Build
cargo build

# Run
cargo run

# Run with debug logging
RUST_LOG=debug cargo run

# Run in release mode
cargo run --release
```

## What It Demonstrates

1. **Sharded Actors**: Multiple actor instances (shards), each handling a subset of keys
2. **Hash-Based Routing**: Partition key → hash → shard_id (consistent routing)
3. **Scatter-Gather**: Query all shards and collect results
4. **PlexSpaces APIs**:
   - `NodeBuilder::new().build().await` - Create node with auto-initialized services
   - `ActorBuilder::new(behavior).spawn(&ctx, service_locator)` - Spawn actor with custom behavior
   - `RequestContext::new_without_auth(tenant, namespace)` - Explicit tenant isolation
   - `Message::json(&data)?.with_message_type("type")` - Create messages
   - `actor_ref.tell(msg).await` - Fire-and-forget messaging

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        Main                                  │
│   partition_key("user-1") → hash → shard_id=3               │
└─────────────────────────────────────────────────────────────┘
                              │
        ┌─────────────────────┼─────────────────────┐
        ▼                     ▼                     ▼
   ┌─────────┐          ┌─────────┐          ┌─────────┐
   │ Shard 0 │          │ Shard 1 │          │ Shard 2 │  ...
   │ user-2  │          │ user-3  │          │ user-4  │
   └─────────┘          └─────────┘          └─────────┘
```

## Key Code Patterns

### Creating Actors with Custom Behavior

```rust
// Define behavior
struct ShardActor {
    shard_id: usize,
    counts: HashMap<String, u64>,
}

#[async_trait]
impl ActorTrait for ShardActor {
    async fn handle_message(&mut self, _ctx: &ActorContext, message: Message) -> Result<(), BehaviorError> {
        let msg: ShardMessage = serde_json::from_slice(&message.payload)?;
        // Handle message...
        Ok(())
    }
    
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// Spawn with ActorBuilder
let actor = ActorBuilder::new(Box::new(ShardActor::new(0)))
    .with_id("shard-0@node")
    .with_namespace("my-namespace")
    .spawn(&ctx, service_locator)
    .await?;
```

### Sending Messages

```rust
// Create message with JSON serialization
let msg = Message::json(&ShardMessage::Increment { key: "user-1".into() })?
    .with_message_type("increment");

// Send (fire-and-forget)
actor.tell(msg).await?;
```

### Hash-Based Routing

```rust
fn route_to_shard(key: &str, shard_count: usize) -> usize {
    let hash = key.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % shard_count as u64) as usize
}
```

## Expected Output

```
╔════════════════════════════════════════════════════════════════╗
║           Actor Groups (Sharding) Example                      ║
╚════════════════════════════════════════════════════════════════╝

Step 1: Creating 4 shard actors...
  ✓ Created shard-0
  ✓ Created shard-1
  ✓ Created shard-2
  ✓ Created shard-3

Step 2: Sending messages (partition key → shard)...
  user-1 → shard-3
  Shard 3: user-1 → 1
  user-2 → shard-0
  Shard 0: user-2 → 1
  ...

Step 3: Scatter-gather query (GetTotal)...
  → Querying shard-0
  ...
  Shard 0: GetTotal → 1

...
```

## Use Cases

**Use Actor Groups (Sharding) when:**
- High-throughput workloads (millions of ops/sec)
- Horizontally scalable state (counters, caches, indexes)
- Partitionable data (user IDs, session IDs, timestamps)
- Eventual consistency is acceptable

**Don't use when:**
- Need broadcast/coordination → Use Process Groups (Pub/Sub)
- Strong consistency required → Use single actor
- Small datasets → Overhead not worth it

## See Also

- [Process Groups (Pub/Sub)](../process_groups_pubsub/) - For broadcast patterns
- [Supervision Tree](../supervision_tree/) - For fault tolerance
- [Architecture Docs](../../../../docs/architecture.md)
