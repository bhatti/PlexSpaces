# Mailbox - Composable Message Queue Abstractions

**Purpose**: Provides composable mailbox implementations for actors, supporting priority-based message delivery, backpressure, deduplication, and a high-priority control-message lane.

## Overview

This crate provides the message queue abstraction used by every actor in PlexSpaces.  Each actor owns one `Mailbox` which internally maintains **two independent queues**:

| Queue | Content | Back-pressure |
|-------|---------|--------------|
| `ctrl_queue` | Control messages (`__DOWN__`, `__EXIT__`, `__PING__`, `__INFO__`, …) | None — unbounded, never blocks |
| `data_queue` | All other application messages (`call`, `cast`, `timer`, …) | Configurable (Error / DropOldest / DropNewest / Block) |

On every `dequeue()` call the ctrl queue is checked first via a single atomic load (`ctrl_size: AtomicUsize`).  When no control messages are pending this is effectively free — one relaxed load + a branch that is always-not-taken at steady state.

## Control-Message Convention

A message is a **control message** when `message_type.starts_with("__")`.

```rust
use plexspaces_core::{is_ctrl_message, CTRL_MSG_PREFIX};

assert!(is_ctrl_message("__DOWN__"));
assert!(is_ctrl_message("__EXIT__"));
assert!(is_ctrl_message("__PING__"));
assert!(!is_ctrl_message("call"));
```

Built-in control messages:

| Type | Direction | Meaning |
|------|-----------|---------|
| `__DOWN__` | node → watcher | Monitored actor terminated |
| `__EXIT__` | node → linked | Linked actor terminated with error |
| `__PING__` | caller → actor | Liveness probe (auto-replied as `__PONG__`) |
| `__PONG__` | actor → caller | Automatic reply to `__PING__` |

### PING / PONG Liveness Probe

`__PING__` is handled automatically by the actor run loop — actor code never sees it.  The loop sends a `__PONG__` reply with the same `correlation_id` so an `ask()` future can match the response:

```rust
use plexspaces_core::create_ping_message;
use std::time::Duration;

// Send a PING and wait for PONG (≤ 100 ms = actor is live)
let ping = create_ping_message(my_actor_id.as_str(), target_actor_id.as_str());
let pong = target_actor_ref.ask(ping, Duration::from_millis(100)).await?;
assert_eq!(pong.message_type, "__PONG__");
```

## Key Components

### Mailbox

```rust
impl Mailbox {
    // Enqueue: ctrl messages go to the ctrl queue; data messages to the data queue
    pub async fn enqueue(&self, message: Message) -> Result<(), MailboxError>;

    // Dequeue: drains ctrl queue first, then data queue
    pub fn dequeue(&self) -> impl Future<Output = Option<Message>>;
    pub fn dequeue_with_timeout(&self, timeout: Option<Duration>) -> impl Future<Output = Option<Message>>;

    // Size helpers (sync — read atomics directly, no await needed)
    pub fn size(&self) -> usize;               // data queue depth (not counting ctrl)
    pub fn ctrl_size(&self) -> usize;          // ctrl queue depth (atomic, O(1))

    // Observability (sync — all counters are atomics)
    pub fn get_stats(&self) -> MailboxObservabilityStats;
}
```

### MailboxObservabilityStats

```rust
pub struct MailboxObservabilityStats {
    pub data_queue_size: usize,   // pending data messages
    pub ctrl_queue_size: usize,   // pending control messages
    pub total_enqueued:  usize,   // lifetime data messages enqueued
    pub total_dequeued:  usize,   // lifetime messages dequeued
    pub total_dropped:   usize,   // messages dropped by DropOldest/DropNewest backpressure
    pub backend_type:    String,  // "in_memory", "sqlite", "redis", etc.
    pub is_durable:      bool,
}

impl MailboxObservabilityStats {
    pub fn total_size(&self) -> usize; // data_queue_size + ctrl_queue_size
}
```

Non-zero `total_dropped` indicates the actor is falling behind its message rate — a signal to scale or tune backpressure strategy.

## Backpressure

Backpressure applies **only to data messages**.  Control messages are never dropped or blocked regardless of how full the data queue is.

```rust
pub enum BackpressureStrategy {
    Block,      // Return MailboxError::Full (default)
    DropOldest, // Drop the oldest data message and accept the new one
    DropNewest, // Silently drop the incoming message
    Error,      // Same as Block
}
```

## Channel Backends

The data queue is backed by a pluggable `Channel` trait:

| Backend | Durable | Use Case |
|---------|---------|----------|
| `InMemory` (default) | No | Development, single-node |
| `SQLite` | Yes | Single-node persistence |
| `Redis` | Yes | Distributed, low-latency |
| `Kafka` | Yes | High-throughput streaming |
| `NATS` | Yes | Cloud-native messaging |

## Metrics (via `metrics` crate)

| Counter | Labels | Description |
|---------|--------|-------------|
| `plexspaces_mailbox_ctrl_enqueued_total` | `mailbox_id` | Ctrl messages enqueued |
| `plexspaces_mailbox_ctrl_dequeued_total` | `mailbox_id`, `message_type` | Ctrl messages dequeued |
| `plexspaces_mailbox_size` | `actor_id`, `backend` | Current total mailbox size (gauge) |

## Usage

### Basic

```rust
use plexspaces_mailbox::{Mailbox, mailbox_config_default, new_message};
use std::time::Duration;

let mailbox = Mailbox::new(mailbox_config_default(), "my-actor".into(), String::new(), String::new(), None).await?;

// Data message
mailbox.enqueue(new_message(b"hello".to_vec())).await?;

// Control message — goes to ctrl queue, returned first
let mut down = new_message(vec![]);
down.message_type = "__DOWN__".into();
mailbox.enqueue(down).await?;

// __DOWN__ arrives before "hello"
let first = mailbox.dequeue().await.unwrap();
assert_eq!(first.message_type, "__DOWN__");
```

### Building with MailboxBuilder

```rust
use plexspaces_mailbox::MailboxBuilder;

let mailbox = MailboxBuilder::new()
    .with_capacity(10_000)
    .build("actor-mailbox".into())
    .await?;
```

## Performance

- **Ctrl-queue enqueue**: ~1 atomic increment + unbounded channel send (no allocation)
- **Ctrl-queue check on dequeue**: one `Relaxed` atomic load; zero mutex cost when empty
- **Data enqueue**: channel send + optional priority-heap insertion
- **Data dequeue**: channel recv (async, zero-copy for InMemory backend)

## Testing

```bash
cargo test -p plexspaces-mailbox --lib
```

## References

- Architecture: [docs/architecture.md](../../docs/architecture.md)
- Detailed design: [docs/detailed-design.md](../../docs/detailed-design.md)
- Control messages: `crates/core/src/actor_monitor.rs` (`CTRL_MSG_PREFIX`, `is_ctrl_message`)
- Durability: [docs/durability.md](../../docs/durability.md)
