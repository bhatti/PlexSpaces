# Example Quality Criteria

## Mandatory Requirements

Every example MUST meet ALL of these criteria:

### 1. Use Correct PlexSpaces APIs

| Feature | Correct API | WRONG (Don't Use) |
|---------|-------------|-------------------|
| Actor creation | `node.spawn()` or `ActorBuilder.spawn()` | Manual `Actor::new()` + `Mailbox::new()` |
| Timers | `TimerFacet.register_once/periodic()` | `tokio::spawn` + `sleep` |
| Reminders | `ReminderFacet` | Custom reminder store |
| Durability | `DurabilityFacet` | Custom journaling |
| Pub/Sub | `ProcessGroupRegistry` | Custom broadcast |
| Coordination | `TupleSpace` | Custom coordination |
| Request/Reply | `ActorRef::ask()` | Manual reply tracking |
| Fire-and-forget | `ActorRef::tell()` | - |
| Message creation | `Message::json()` | Manual serialization |
| Config loading | `ConfigBootstrap` | Manual file reading |
| Supervision | `Supervisor` | Custom restart logic |

### 2. File Structure

```
example_name/
├── .cargo/
│   └── config.toml      # target-dir = "../../../../target"
├── Cargo.toml           # Minimal dependencies
├── README.md            # REQUIRED - see below
├── release.toml         # Config (if using ConfigBootstrap)
└── src/
    └── main.rs          # Single file preferred
```

### 3. README Requirements

Every README MUST include:

```markdown
# Example Name

**Purpose**: One sentence describing what this demonstrates.

**PlexSpaces APIs**: List of APIs used (e.g., `TimerFacet`, `ProcessGroupRegistry`)

## Quick Start

\`\`\`bash
cd examples/rust/embedded/example_name
cargo build
cargo run
\`\`\`

## What It Demonstrates

1. Feature 1
2. Feature 2
3. Feature 3

## PlexSpaces API Usage

### API 1
\`\`\`rust
// Code showing how to use the API
\`\`\`

## Use Cases

- Real-world use case 1
- Real-world use case 2

## See Also

- [Related Example](../related/)
```

### 4. Code Quality

- [ ] Compiles without errors
- [ ] Runs successfully
- [ ] No unnecessary dependencies
- [ ] No unnecessary tests (remove test folders unless critical)
- [ ] Clear output showing what's happening
- [ ] Comments explaining PlexSpaces API usage
- [ ] Real-world use case (not abstract API demo)

### 5. Simplicity

- Single `main.rs` file preferred
- No lib.rs unless truly needed
- No complex module structure
- No Docker files
- No integration test folders unless critical
- Minimal dependencies

---

## Example Checklist

| # | Example | APIs | README | Builds | Runs | Simple |
|---|---------|------|--------|--------|------|--------|
| 1 | actor_groups_sharding | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 2 | supervision_tree | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 3 | durable_actor | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 4 | timers | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 5 | reminders | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 6 | chat_room | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 7 | feature_flags | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 8 | webhook_handler | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 9 | heat_diffusion | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 10 | matrix_multiply | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 11 | mpi_collectives | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |
| 12 | byzantine | ⬜ | ⬜ | ⬜ | ⬜ | ⬜ |

---

## API Quick Reference

### Actor Creation (use node.spawn)
```rust
let actor = node.spawn(&ctx, &actor_id, "ActorType", initial_state, None, HashMap::new(), vec![]).await?;
```

### ActorBuilder (alternative)
```rust
let actor = ActorBuilder::new(Box::new(MyActor::new()))
    .with_id("my-actor@node")
    .with_namespace("namespace")
    .spawn(&ctx, service_locator)
    .await?;
```

### TimerFacet
```rust
let timer_facet = TimerFacet::new(json!({}), 50);
actor.attach_facet(Box::new(timer_facet)).await?;

// Register timers
facet.register_once("timeout", Duration::from_secs(30)).await?;
facet.register_periodic("heartbeat", Duration::from_secs(5)).await?;
facet.cancel("timeout").await?;
```

### ReminderFacet
```rust
let reminder_facet = ReminderFacet::new(storage, json!({}), 50);
actor.attach_facet(Box::new(reminder_facet)).await?;

// Schedule durable reminders
facet.schedule("billing", Duration::from_days(30), data).await?;
```

### ProcessGroupRegistry
```rust
let registry = ProcessGroupRegistry::new("node", kv_store);
registry.create_group(&ctx, "group-name").await?;
registry.join_group(&ctx, "group-name", &actor_id, topics).await?;
registry.publish_to_group(&ctx, "group-name", None, data).await?;
```

### TupleSpace
```rust
let ts = TupleSpace::with_tenant_namespace("tenant", "namespace");
ts.write(&ctx, tuple).await?;
ts.read(&ctx, pattern).await?;
ts.take(&ctx, pattern).await?;
```

### Supervisor
```rust
let (supervisor, event_rx) = Supervisor::new(
    "supervisor-name",
    SupervisionStrategy::OneForOne { max_restarts: 5, within_seconds: 60 },
    service_locator,
);
supervisor.add_child(child_spec).await?;
```

### Message
```rust
let msg = Message::json(&data)?.with_message_type("type");
actor_ref.tell(msg).await?;
let reply = actor_ref.ask(msg, timeout).await?;
```

### ConfigBootstrap
```rust
let config: MyConfig = ConfigBootstrap::load().unwrap_or_default();
```
