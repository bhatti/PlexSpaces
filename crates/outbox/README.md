# plexspaces-outbox

Flow-controlled messaging for PlexSpaces — sender-side outbox relay with AIMD concurrency control; bounded mailbox provides the receiver-side backpressure gate.

## Purpose

When a PlexSpaces actor needs to send messages to millions of targets, doing so naively exhausts memory/CPU on the sender and overwhelms receivers. This crate provides local-only flow control (no Kafka/SQS required) using actors + DB-backed channels.

## Components

| Component | Role | Location |
|-----------|------|----------|
| `ConcurrencyLimiter` | AIMD concurrency control (atomic, lock-free) — reusable for any outbound throttling | Sender-side |
| `OutboxRelay` | Async drain loop: channel → permits → delivery → ack/nack | Sender-side |
| `PartitionedOutboxRelay` | Sharded relay: FNV-1a hash → N partitioned OutboxRelay instances | Sender-side |
| `HashRouter` | Deterministic partition assignment by FNV-1a hash of target actor ID | Sender-side |
| Dispatcher helpers | Proto encode/decode, config validation, status reporting | Shared |

## Architecture

```
SenderActor → outbox.dispatch() → OutboxRelay (AIMD concurrency, DB-backed channel)
                                    ├─ Channel (SQLite/Postgres, existing ChannelConfig)
                                    └─ drain loop → tell() → TargetActor (bounded mailbox)
                                                              MailboxFull → DeliveryError::Capacity
                                                              → AIMD on_rejection() → limit halves
```

Backpressure is provided by the **bounded mailbox** at enqueue time: when a target actor
can't keep up, `tell()` returns `MailboxFull` which maps to `DeliveryError::Capacity` and
triggers AIMD multiplicative decrease. Use `ConcurrencyLimiter` directly for any outbound
throttling scenario outside of `OutboxRelay`.

## Dependencies

- `plexspaces-proto` (L0) — outbox proto types
- `plexspaces-common` (L1) — shared retry utility (`compute_backoff`, `should_retry`)
- `plexspaces-channel` (L4) — `Channel` trait for durable storage

## References

- [Architecture — Flow Control](../../docs/architecture.md)
- [Detailed Design — Flow-Controlled Messaging](../../docs/detailed-design.md)
- [Job Queue Example](../../examples/rust/embedded/job_queue/)
