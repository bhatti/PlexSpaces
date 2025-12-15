# Cloudflare Workers Durable Objects vs PlexSpaces Comparison

This comparison demonstrates how to implement Durable Objects (stateful edge computing) in both Cloudflare Workers and PlexSpaces.

## Use Case: Counter Durable Object

A stateful counter that:
- Maintains state across requests
- Automatically persists state
- Handles concurrent requests
- Demonstrates VirtualActorFacet and durable state

## PlexSpaces Abstractions Showcased

This example demonstrates:
- ✅ **VirtualActorFacet** - Automatic activation/deactivation
- ✅ **DurabilityFacet** - Automatic state persistence
- ✅ **Actor Model** - Stateful computation with location transparency

## Design Decisions

**Why VirtualActorFacet?**
- Matches Cloudflare Durable Objects' automatic activation/deactivation
- Actors are activated on-demand and deactivated when idle
- State is automatically persisted

**Why DurabilityFacet?**
- Durable Objects automatically persist state
- PlexSpaces makes this optional via DurabilityFacet
- Enables exactly-once semantics and state recovery

---

## Cloudflare Workers Implementation

### Native TypeScript Code

```typescript
// counter.ts - Durable Object class
export class Counter {
    private state: DurableObjectState;
    private count: number = 0;

    constructor(state: DurableObjectState, env: Env) {
        this.state = state;
        // Load state from storage
        this.state.storage.get<number>("count").then((value) => {
            this.count = value || 0;
        });
    }

    async fetch(request: Request): Promise<Response> {
        const url = new URL(request.url);
        const action = url.pathname.split("/").pop();

        switch (action) {
            case "increment":
                this.count++;
                // Persist state
                await this.state.storage.put("count", this.count);
                return new Response(JSON.stringify({ count: this.count }));

            case "decrement":
                this.count = Math.max(0, this.count - 1);
                await this.state.storage.put("count", this.count);
                return new Response(JSON.stringify({ count: this.count }));

            case "get":
                return new Response(JSON.stringify({ count: this.count }));

            default:
                return new Response("Not found", { status: 404 });
        }
    }
}

// worker.ts - Cloudflare Worker
export default {
    async fetch(request: Request, env: Env): Promise<Response> {
        // Get Durable Object ID from URL
        const id = env.COUNTER.idFromName("counter-1");
        const stub = env.COUNTER.get(id);
        
        // Forward request to Durable Object
        return stub.fetch(request);
    },
};

// wrangler.toml
// [durable_objects]
// bindings = [
//   { name = "COUNTER", class_name = "Counter" }
// ]
```

### Running Cloudflare Workers Example

```bash
# Install Wrangler CLI
npm install -g wrangler

# Deploy
wrangler deploy

# Test
curl https://your-worker.workers.dev/increment
curl https://your-worker.workers.dev/get
```

---

## PlexSpaces Implementation

### Rust Implementation

See `src/main.rs` for the complete implementation.

### Key Differences

| Feature | Cloudflare Workers | PlexSpaces |
|---------|-------------------|------------|
| **Language** | TypeScript | Rust |
| **Activation** | Automatic (Durable Objects) | VirtualActorFacet (optional) |
| **State Persistence** | Automatic (storage API) | DurabilityFacet (optional) |
| **Location** | Edge (Cloudflare network) | Any node (location-transparent) |
| **Distribution** | Cloudflare network | gRPC + ActorRef |

### Architecture Comparison

**Cloudflare Workers**:
```
Worker (TypeScript)
  └─ Durable Object (TypeScript class)
      └─ Storage API (automatic persistence)
```

**PlexSpaces**:
```
Actor (Rust)
  └─ VirtualActorFacet (automatic activation/deactivation)
      └─ DurabilityFacet (optional state persistence)
```

---

## Side-by-Side Code Comparison

### Counter Implementation

**Cloudflare Workers (TypeScript)**:
```typescript
export class Counter {
    private count: number = 0;

    async fetch(request: Request): Promise<Response> {
        const action = new URL(request.url).pathname.split("/").pop();
        
        if (action === "increment") {
            this.count++;
            await this.state.storage.put("count", this.count);
            return new Response(JSON.stringify({ count: this.count }));
        }
        // ...
    }
}
```

**PlexSpaces (Rust)**:
```rust
pub struct CounterActor {
    count: u64,
}

#[async_trait]
impl ActorBehavior for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        match msg.payload() {
            b"increment" => {
                self.count += 1;
                // State automatically persisted via DurabilityFacet
                ctx.reply(Message::new(serde_json::to_vec(&self.count)?)).await?;
            }
            // ...
        }
        Ok(())
    }
}
```

### State Persistence

**Cloudflare Workers (TypeScript)**:
```typescript
// Automatic - state.storage is always available
await this.state.storage.put("count", this.count);
const count = await this.state.storage.get<number>("count");
```

**PlexSpaces (Rust)**:
```rust
// Optional - only if DurabilityFacet is attached
let actor = ActorBuilder::new()
    .with_behavior(CounterActor::new())
    .with_virtual_actor(VirtualActorConfig::default())
    .with_durability(DurabilityConfig::default())
    .build();
```

---

## Running the Comparison

### Prerequisites

```bash
# For Cloudflare Workers example (optional)
npm install -g wrangler

# For PlexSpaces example
cargo build --release
```

### Run PlexSpaces Example

```bash
# Run the example
cargo run --release

# Run tests
cargo test

# Run test script
./scripts/test.sh
```

---

## Performance Comparison

### Benchmarks

| Metric | Cloudflare Workers | PlexSpaces | Notes |
|--------|-------------------|------------|-------|
| **Request Latency** | <10ms | <5ms | Edge vs local |
| **State Persistence** | <5ms | <2ms | With DurabilityFacet |
| **Cold Start** | <50ms | <20ms | Actor activation |
| **Throughput** | 100K+ req/s | 200K+ req/s | Single node |
| **Global Distribution** | ✅ Built-in | ✅ Multi-node | Different model |

*Note: Benchmarks are approximate. See `metrics/benchmark_results.json` for detailed results.*

---

## Feature Comparison

| Feature | Cloudflare Workers | PlexSpaces | Notes |
|---------|-------------------|------------|-------|
| **Durable State** | ✅ Automatic | ✅ DurabilityFacet | Optional in PlexSpaces |
| **Automatic Activation** | ✅ Built-in | ✅ VirtualActorFacet | Similar |
| **Edge Distribution** | ✅ Global network | ⚠️ Multi-node | Different model |
| **Location Transparency** | ✅ Built-in | ✅ ActorRef | Similar |
| **State Recovery** | ✅ Automatic | ✅ Journaling | Similar |
| **Concurrent Requests** | ✅ Handled | ✅ Actor mailbox | Similar |

---

## When to Use Each

### Use Cloudflare Workers When:
- ✅ Need global edge distribution
- ✅ Building TypeScript/JavaScript applications
- ✅ Want Cloudflare's CDN integration
- ✅ Need automatic global distribution

### Use PlexSpaces When:
- ✅ Need Rust performance
- ✅ Want optional durability (pay for what you use)
- ✅ Need unified actor + workflow model
- ✅ Want proto-first contracts
- ✅ Need WASM support
- ✅ Want Firecracker isolation
- ✅ Need multi-cloud deployment

---

## Design Decisions Explained

### Why VirtualActorFacet?

**Cloudflare Workers Approach**: Durable Objects are automatically activated on first request, deactivated when idle.

**PlexSpaces Approach**: VirtualActorFacet provides the same automatic activation/deactivation, but it's optional.

**Rationale**:
- **Flexibility**: Not all actors need virtual actor lifecycle
- **Performance**: Actors without VirtualActorFacet have zero overhead
- **Composition**: Can combine with other facets (DurabilityFacet, TimerFacet, etc.)

### Why DurabilityFacet?

**Cloudflare Workers Approach**: Durable Objects automatically persist state via storage API.

**PlexSpaces Approach**: DurabilityFacet makes state persistence optional.

**Rationale**:
- **Cost**: State persistence has storage costs - only use when needed
- **Performance**: Actors without durability have zero overhead
- **Flexibility**: Can mix durable and non-durable actors

---

## References

- [Cloudflare Durable Objects Documentation](https://developers.cloudflare.com/durable-objects/)
- [PlexSpaces VirtualActorFacet](../../../../crates/facet)
- [PlexSpaces DurabilityFacet](../../../../crates/journaling)
- [PlexSpaces Actor Model](../../../../crates/actor)
