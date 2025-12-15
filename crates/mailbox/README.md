# Mailbox - Composable Message Queue Abstractions

**Purpose**: Provides composable mailbox implementations for actors, supporting priority-based message delivery, backpressure, and async message handling.

## Overview

This crate provides message queue abstractions for actors. Mailboxes are the communication mechanism between actors, enabling asynchronous message passing with various delivery guarantees and priorities.

## Key Components

### Mailbox

Main mailbox abstraction:

```rust
pub struct Mailbox {
    config: MailboxConfig,
    queue: Arc<RwLock<MessageQueue>>,
}

impl Mailbox {
    pub async fn send(&self, message: Message) -> Result<(), MailboxError>;
    pub async fn receive(&self) -> Result<Message, MailboxError>;
    pub async fn try_receive(&self) -> Result<Option<Message>, MailboxError>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
}
```

### Message

Message type with metadata:

```rust
pub struct Message {
    id: String,
    payload: Vec<u8>,
    sender: Option<String>,
    priority: Priority,
    timestamp: SystemTime,
    metadata: HashMap<String, String>,
    idempotency_key: Option<String>,  // For message deduplication
}
```

**Idempotency Keys**: Messages can include an optional `idempotency_key` for deduplication. The mailbox uses an LRU cache to track processed idempotency keys within a configurable time window, preventing duplicate message processing.

### MailboxConfig

Configuration for mailbox behavior:

```rust
pub struct MailboxConfig {
    max_size: Option<usize>,
    priority_enabled: bool,
    backpressure_strategy: BackpressureStrategy,
}
```

## Features

### Priority-Based Delivery

Messages can have priorities (High, Normal, Low):

```rust
let message = Message::new(b"data".to_vec())
    .with_priority(Priority::High);
mailbox.send(message).await?;
```

### Backpressure Support

Mailbox can handle backpressure when full:

```rust
pub enum BackpressureStrategy {
    Block,      // Block sender until space available
    DropOldest, // Drop oldest message
    DropNewest, // Drop newest message
    Reject,     // Reject new messages
}
```

### Async Message Handling

Non-blocking message operations:

```rust
// Non-blocking receive
if let Some(msg) = mailbox.try_receive().await? {
    // Process message
}

// Blocking receive (waits for message)
let msg = mailbox.receive().await?;
```

### Idempotency and Deduplication

Messages with `idempotency_key` are automatically deduplicated:

```rust
use plexspaces_mailbox::Message;

// Send message with idempotency key
let message = Message::new(b"data".to_vec())
    .with_idempotency_key("unique-request-id-123");

mailbox.enqueue(message.clone()).await?;

// Duplicate message with same idempotency key is skipped
mailbox.enqueue(message).await?;  // Deduplicated, not processed again
```

**Implementation**: The mailbox uses an LRU cache with configurable size and TTL to track idempotency keys. Duplicate messages within the time window are automatically skipped.

### Metrics and Monitoring

Built-in hooks for metrics:

```rust
pub trait MailboxMetrics {
    fn on_message_sent(&self, priority: Priority);
    fn on_message_received(&self, priority: Priority);
    fn on_backpressure(&self, strategy: BackpressureStrategy);
}
```

## Usage Examples

### Basic Mailbox Usage

```rust
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};

let mailbox = Mailbox::new(MailboxConfig::default());

// Send message
let message = Message::new(b"hello".to_vec());
mailbox.send(message).await?;

// Receive message
let received = mailbox.receive().await?;
```

### Priority Mailbox

```rust
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message, Priority};

let config = MailboxConfig {
    priority_enabled: true,
    max_size: Some(1000),
    backpressure_strategy: BackpressureStrategy::Block,
};
let mailbox = Mailbox::new(config);

// Send high-priority message
let urgent = Message::new(b"urgent".to_vec())
    .with_priority(Priority::High);
mailbox.send(urgent).await?;

// Send normal-priority message
let normal = Message::new(b"normal".to_vec())
    .with_priority(Priority::Normal);
mailbox.send(normal).await?;
```

### Backpressure Handling

```rust
use plexspaces_mailbox::{Mailbox, MailboxConfig, BackpressureStrategy};

let config = MailboxConfig {
    max_size: Some(100),
    backpressure_strategy: BackpressureStrategy::DropOldest,
    ..Default::default()
};
let mailbox = Mailbox::new(config);

// If mailbox is full, oldest message will be dropped
for i in 0..200 {
    let msg = Message::new(format!("msg{}", i).into_bytes());
    mailbox.send(msg).await?; // May drop older messages
}
```

## Performance Characteristics

- **Message send**: < 1μs (in-memory)
- **Message receive**: < 1μs (in-memory)
- **Priority sorting**: O(log n) for priority queue
- **Memory per message**: ~100 bytes (metadata overhead)

## Testing

```bash
# Run all mailbox tests
cargo test -p plexspaces-mailbox

# Check coverage
cargo tarpaulin -p plexspaces-mailbox --out Html
```

## Dependencies

This crate depends on:
- `tokio`: Async runtime
- `serde`: Serialization
- `uuid`: Message ID generation

## Dependents

This crate is used by:
- `plexspaces_actor`: Actors use Mailbox for message delivery
- `plexspaces_core`: Re-exports Mailbox and Message types
- All other crates: For message passing

## References

- Implementation: `crates/mailbox/src/`
- Tests: `crates/mailbox/src/` (unit tests)

