# Byzantine Generals - Consensus Example

**Purpose**: Demonstrate Byzantine Fault Tolerant consensus using the PlexSpaces Application framework.

## PlexSpaces Features Demonstrated

| Feature | Usage |
|---------|-------|
| **Application** | `ByzantineApplication` implements `Application` trait |
| **ConfigBootstrap** | Load config from `release.toml` |
| **BehaviorRegistry** | Register `ByzantineGeneral` behavior |
| **SDK Spawn Helper** | Spawn general actors from registered behavior types |
| **ActorContext** | Message passing between generals |
| **ActorRef::ask()** | Request-reply pattern for results |
| **GenServer** | General actors implement GenServer behavior |

## Quick Start

```bash
cd examples/rust/embedded/byzantine

# Build
cargo build

# Run with default config (4 generals, 1 byzantine)
cargo run

# Run with custom config
GENERAL_COUNT=7 FAULT_COUNT=2 cargo run
```

## Configuration

Edit `release.toml`:

```toml
# Total number of generals (minimum 4)
general_count = 4

# Number of Byzantine (faulty) generals (must be < general_count/3)
fault_count = 1

# TupleSpace backend: "memory", "redis", or "postgres"
tuplespace_backend = "memory"
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    ByzantineApplication                         │
│  - Implements Application trait                                 │
│  - Loads config via ConfigBootstrap                             │
│  - Registers behaviors via BehaviorRegistry                     │
│  - Spawns actors via SDK helper on top of Node services         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                         Node                                    │
│  - ServiceLocator provides ActorService, ActorRegistry          │
│  - Manages actor lifecycle                                      │
└─────────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
    ┌──────────┐        ┌──────────┐        ┌──────────┐
    │ General0 │        │ General1 │        │ General2 │
    │ (source) │        │ (honest) │        │ (faulty) │
    └────┬─────┘        └────┬─────┘        └────┬─────┘
         │                   │                   │
         └───────────────────┼───────────────────┘
                             │
                    ActorService.send()
                    ActorRef::ask()
```

## Key Code Patterns

### 1. Application Definition

```rust
pub struct ByzantineApplication {
    config: ByzantineConfig,
}

#[async_trait]
impl Application for ByzantineApplication {
    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        // Get ServiceLocator from node
        let service_locator = node.service_locator()?;
        
        // Register behaviors
        let mut behavior_registry = BehaviorRegistry::new();
        register_byzantine_behaviors(&mut behavior_registry, journal, tuplespace).await;
        service_locator.register_service(Arc::new(behavior_registry)).await;
        
        // Spawn actors via SDK helper using registered behavior type
        spawn_with_behavior_type(&ctx, service_locator.clone(), actor_id, "consensus", "ByzantineGeneral", initial_state, vec![]).await?;
        
        // Run algorithm
        algorithm.run(&ctx).await?;
        
        Ok(())
    }
}
```

### 2. Config Loading

```rust
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ByzantineConfig {
    pub general_count: usize,
    pub fault_count: usize,
    pub tuplespace_backend: String,
}

impl ByzantineConfig {
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }
}
```

### 3. Behavior Registration

```rust
pub async fn register_byzantine_behaviors(
    registry: &mut BehaviorRegistry,
    _journal: Arc<dyn Journal>,
    _tuplespace: Arc<TupleSpace>,
) {
    registry.register("ByzantineGeneral", move |initial_state: &[u8]| {
        let state: Value = serde_json::from_slice(initial_state)?;
        let general = General::new(id, source_id, num_rounds);
        Ok(Box::new(general) as Box<dyn Actor>)
    }).await;
}
```

### 4. Actor Implementation

```rust
pub struct General {
    id: usize,
    source_id: usize,
    values: HashMap<String, Value>,
    is_faulty: bool,
}

#[async_trait]
impl Actor for General {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        match deserialize(&msg.payload)? {
            GeneralMessage::Init { general_ids, .. } => self.handle_init(general_ids),
            GeneralMessage::ReceiveMessage { path, value } => self.handle_receive(path, value),
            GeneralMessage::SendMessages { round } => self.send_messages(round, ctx).await?,
            GeneralMessage::GetResult => {
                let result = self.create_result();
                ctx.send_reply(correlation_id, sender, receiver, reply).await?;
            }
        }
        Ok(())
    }
}
```

### 5. Request-Reply Pattern

```rust
// Use ActorRef::ask() to get results
let actor_ref = ActorRef::remote(actor_id, node_id, service_locator);
let reply = actor_ref.ask(message, Duration::from_secs(5)).await?;
let result: GeneralResult = bincode::deserialize(reply.payload())?;
```

## Expected Output

```
╔════════════════════════════════════════════════════════════╗
║     Byzantine Generals - Consensus Example                ║
╚════════════════════════════════════════════════════════════╝

Configuration:
  Generals: 4
  Byzantine (faulty): 1
  TupleSpace backend: memory

🚀 Starting Byzantine Generals Application
   Generals: 4
   Byzantine: 1

Results:
Time: 215ms
Messages: 12
Source Process 0 is faulty
Process 1 decides on value zero
Process 2 is faulty
Process 3 decides on value zero
✅ Byzantine Generals Application started

Running consensus...

🛑 Stopping Byzantine Generals Application
✅ Byzantine Generals Application stopped

╔════════════════════════════════════════════════════════════╗
║                    Example Complete                        ║
╚════════════════════════════════════════════════════════════╝
```

## Use Cases

- **Distributed consensus**: Agreement despite faulty nodes
- **Blockchain validation**: Block proposal and voting
- **Replicated state machines**: Consistent state across replicas
- **Leader election**: Agree on leader despite failures

## See Also

- [Chat Room](../chat_room/) - ProcessGroup broadcast
- [MPI Collectives (Go WASM)](../../../go/apps/mpi_collectives/) - Collective operations with shard-group APIs
- [Architecture Docs](../../../../docs/architecture.md)
