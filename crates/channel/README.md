# PlexSpaces Channel - Extensible Channel/Queue Abstraction

**Purpose**: Provides an extensible channel/queue abstraction for microservices communication, supporting both in-process (Go-like channels) and inter-process (Redis Streams, Kafka) messaging patterns.

## Overview

This crate is central to the PlexSpaces microservices framework, enabling:
- **Worker Queues**: Distribute work across elastic actor pools
- **Pub/Sub**: Event notification for GenEvent-style messaging
- **Request/Reply**: RPC-style communication patterns
- **Streaming**: Data pipelines with backpressure control

## Architecture

```text
┌─────────────────────────────────────────────────────┐
│              PlexSpaces Microservices                │
├─────────────────────────────────────────────────────┤
│                                                       │
│  ┌───────────┐     ┌─────────────┐     ┌─────────┐ │
│  │  Elastic  │────▶│   Channel   │────▶│ Circuit │ │
│  │   Pool    │     │ (This Crate)│     │ Breaker │ │
│  └───────────┘     └─────────────┘     └─────────┘ │
│        │                   │                  │      │
│        ▼                   ▼                  ▼      │
│  ┌────────────────────────────────────────────────┐ │
│  │           Channel Backends                     │ │
│  ├────────────────────────────────────────────────┤ │
│  │ InMemory  │  Redis Streams  │  Kafka  │  NATS │ │
│  │ (MPSC)    │  (Distributed)  │(Streaming)│(Pub/Sub)│ │
│  └────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
```

## Channel Trait

The core abstraction is the `Channel` trait:

```rust
pub trait Channel: Send + Sync {
    async fn send(&self, message: Vec<u8>) -> Result<(), ChannelError>;
    async fn receive(&self) -> Result<Vec<u8>, ChannelError>;
    async fn try_receive(&self) -> Result<Option<Vec<u8>>, ChannelError>;
}
```

## Backend Implementations

### 1. In-Memory Channel (`src/in_memory.rs`)

**Use Case**: Single-node, high-performance messaging
- **Implementation**: `tokio::sync::mpsc` channels
- **Performance**: < 1μs latency
- **Limitations**: Not distributed, lost on node restart

### 2. Redis Backend (`src/redis_backend.rs`)

**Use Case**: Distributed messaging with Redis Streams
- **Implementation**: Redis Streams (XADD, XREAD, XACK)
- **Features**: Consumer groups, message acknowledgment, persistence
- **Performance**: < 5ms latency (network dependent)

### 3. Kafka Backend (`src/kafka_backend.rs`)

**Use Case**: High-throughput streaming pipelines
- **Implementation**: Apache Kafka (rdkafka)
- **Features**: Partitioning, replication, exactly-once semantics
- **Performance**: < 10ms latency, high throughput

### 4. NATS Backend (`src/nats_backend.rs`)

**Use Case**: Lightweight distributed pub/sub messaging
- **Implementation**: NATS (async-nats)
- **Features**: Pub/sub, queue groups for load balancing, optional JetStream persistence
- **Performance**: < 1ms latency, > 1M msgs/sec throughput

## Integration Tests

### Prerequisites

- Docker and Docker Compose installed
- Rust toolchain installed

### Running Integration Tests

```bash
# Start Redis and Kafka
docker-compose up -d

# Wait for health checks
docker-compose ps

# Verify Redis is ready
docker exec plexspaces-channel-redis redis-cli ping
# Should output: PONG

# Verify Kafka is ready
docker exec plexspaces-channel-kafka kafka-topics --bootstrap-server localhost:9092 --list

# Run all tests (including integration tests)
cargo test --all-features

# Run only Redis tests
cargo test --features redis-backend redis

# Run only Kafka tests
cargo test --features kafka-backend kafka

# Run only NATS tests (unit tests - no server required)
cargo test --features nats-backend --lib -p plexspaces-channel

# Run NATS integration tests (requires NATS server, run manually)
docker-compose up -d nats
cargo test --features nats-backend --test nats_integration_test -- --ignored

# Run with coverage
cargo tarpaulin --all-features --out Html
open tarpaulin-report.html

# Stop infrastructure
docker-compose down
```

### Environment Variables

Integration tests check for running services:
- `REDIS_URL`: Default `redis://localhost:6379`
- `KAFKA_BROKERS`: Default `localhost:9092`

If services are not available, integration tests are skipped.

## Usage Examples

### In-Memory Channel

```rust
use plexspaces_channel::{Channel, InMemoryChannel};

let channel = InMemoryChannel::new();
channel.send(b"hello".to_vec()).await?;
let message = channel.receive().await?;
```

### Redis Channel

```rust
use plexspaces_channel::{Channel, RedisChannel};

let channel = RedisChannel::new("redis://localhost:6379", "my-channel").await?;
channel.send(b"hello".to_vec()).await?;
let message = channel.receive().await?;
```

### Kafka Channel

```rust
use plexspaces_channel::{Channel, KafkaChannel};

let channel = KafkaChannel::new("localhost:9092", "my-topic").await?;
channel.send(b"hello".to_vec()).await?;
let message = channel.receive().await?;
```

## Test Organization

- **Unit tests**: In `src/*` files, test trait implementations
- **Integration tests**: In `tests/` directory, test with real Redis/Kafka
- **Docker**: `docker-compose.yml` provides local test infrastructure

## Future Enhancements

- **Hazelcast Backend**: Distributed pub/sub and queues (deferred)
- **RabbitMQ Backend**: AMQP support
- **SQS Backend**: AWS SQS integration
- **JetStream Integration**: Full JetStream support for NATS persistence and at-least-once delivery
- **Latency Tracking**: Implement latency metrics for all backends
- **Throughput Calculation**: Implement throughput metrics for all backends
- **Dead Letter Queue (DLQ)**: Full DLQ support across all backends

