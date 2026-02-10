# Player Session Manager

**Real-World Use Case**: Game server managing millions of player sessions with automatic activation/deactivation and state persistence (Orleans-style Virtual Actor).

## Quick Start

```bash
cd examples/rust/embedded/player_session
cargo run
```

## What It Demonstrates

1. **Virtual Actor Pattern** - One actor per entity (Orleans/Dapr style)
2. **Auto-Activation** - Actor created/restored on first message
3. **Auto-Deactivation** - Actor passivated after idle timeout
4. **State Persistence** - State saved on deactivation, restored on activation
5. **Single Instance** - Only one instance per player across cluster
6. **Timer Facet** - Idle timeout detection

## Virtual Actor Pattern

```
┌─────────────────────────────────────────────────────────────────┐
│                    Virtual Actor Lifecycle                       │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   First Message                                                  │
│        │                                                         │
│        ▼                                                         │
│   ┌─────────┐    Load State    ┌─────────────┐                  │
│   │ Activate │ ───────────────▶│ Process Msgs │◀─────┐          │
│   └─────────┘                  └─────────────┘       │          │
│                                      │               │          │
│                                 Idle Timeout    More Messages   │
│                                      │               │          │
│                                      ▼               │          │
│                               ┌─────────────┐       │          │
│                               │ Save State  │───────┘          │
│                               └─────────────┘                   │
│                                      │                          │
│                                      ▼                          │
│                               ┌─────────────┐                   │
│                               │ Deactivate  │                   │
│                               └─────────────┘                   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                      PlayerSession Actor                         │
├─────────────────────────────────────────────────────────────────┤
│  State: PlayerState                                              │
│    - player_id, username, position                              │
│    - stats (level, xp, health, gold, kills)                     │
│    - inventory[], achievements[]                                 │
│    - last_login, last_activity, session_start                   │
│                                                                  │
│  Handlers:                                                       │
│    #[init_handler]           → Called on activation             │
│    #[handler("login")]       → Player login                     │
│    #[handler("move")]        → Update position                  │
│    #[handler("add_item")]    → Add to inventory                 │
│    #[handler("remove_item")] → Remove from inventory            │
│    #[handler("update_stats")]→ Level up, gain XP/gold           │
│    #[handler("get_state")]   → Query full state                 │
│    #[handler("logout")]      → Explicit deactivation            │
│                                                                  │
│  Facets:                                                         │
│    TimerFacet → Idle timeout detection                          │
│    DurabilityFacet → State persistence (optional)               │
└─────────────────────────────────────────────────────────────────┘
```

## SDK Pattern

```rust
use plexspaces_sdk::*;

// 1. Define virtual actor
#[gen_server_actor]
struct PlayerSession {
    state: PlayerState,
    idle_timeout_seconds: u64,
}

// 2. Add handlers
#[plexspaces_handlers(gen_server)]
impl PlayerSession {
    // Called on activation (load state from storage)
    #[init_handler]
    async fn on_activate(&mut self, ctx: &ActorContext) -> Result<(), BehaviorError> {
        // Load state from database
        // Register idle timeout timer
    }
    
    // Handle login
    #[handler("login")]
    async fn handle_login(&mut self, ctx: &ActorContext, msg: &Message) 
        -> Result<Value, BehaviorError> {
        self.state.username = /* from msg */;
        self.state.is_online = true;
        Ok(json!({ "status": "logged_in" }))
    }
    
    // Handle logout (explicit deactivation)
    #[handler("logout")]
    async fn handle_logout(&mut self, ctx: &ActorContext, msg: &Message) 
        -> Result<Value, BehaviorError> {
        // Save state to storage
        // Trigger deactivation
    }
}

// 3. Spawn virtual actor (one per player)
let player_ref = spawn_actor(&ctx, service_locator, 
    "player-123@game-server",  // Actor ID = Player ID
    "players",
    PlayerSession::new("player-123"),
    vec![Box::new(timer_facet)]).await?;

// 4. Send messages (auto-activates if not active)
let msg = Message {
    id: ulid::Ulid::new().to_string(),
    message_type: "call".to_string(),
    payload: serde_json::to_vec(&json!({
        "op": "login",
        "username": "DragonSlayer42"
    }))?,
    ..Default::default()
};
player_ref.ask(msg, timeout).await?;
```

## Key APIs

| API | Purpose |
|-----|---------|
| `#[gen_server_actor]` | Mark struct as GenServer actor |
| `#[init_handler]` | Called on actor activation |
| `#[handler("op")]` | Handle specific operations |
| `TimerFacet` | Idle timeout detection |
| `DurabilityFacet` | State persistence |

## Virtual Actor vs Regular Actor

| Feature | Virtual Actor | Regular Actor |
|---------|--------------|---------------|
| Lifecycle | Auto-managed | Manual |
| Activation | On first message | Explicit spawn |
| Deactivation | Idle timeout | Explicit stop |
| State | Persisted | In-memory only |
| Instance | Single per ID | Multiple allowed |
| Scaling | Millions | Thousands |

## Durability Options

### Without Durability (In-Memory)
```rust
#[gen_server_actor]
struct PlayerSession { ... }
```
- State lost on deactivation
- Fast, no I/O overhead
- Good for: Caches, sessions, temporary state

### With Durability (Persistent)
```rust
#[gen_server_actor(facets = ["durability"])]
struct PlayerSession { ... }
```
- State persisted to journal
- Survives node crashes
- Good for: Game saves, user profiles, accounts

## Use Cases

- **Gaming**: Player sessions, game state, leaderboards
- **IoT**: Device twins (Azure IoT Hub pattern)
- **E-Commerce**: Shopping carts, user sessions
- **Finance**: Bank accounts, portfolios
- **Social**: User profiles, presence status
- **Logistics**: Package tracking, vehicle state

## Orleans Comparison

| Orleans | PlexSpaces |
|---------|------------|
| `IGrain` | `#[gen_server_actor]` |
| `GrainFactory.GetGrain<T>(id)` | `spawn_actor(..., id, ...)` |
| `[Reentrant]` | Default behavior |
| `[StatelessWorker]` | Worker pool pattern |
| `IPersistentState<T>` | `DurabilityFacet` |
| `IRemindable` | `ReminderFacet` |

## See Also

- [SDK Documentation](../../../../docs/sdk.md)
- [Getting Started](../../../../docs/getting-started.md)
- [Architecture](../../../../docs/architecture.md)
