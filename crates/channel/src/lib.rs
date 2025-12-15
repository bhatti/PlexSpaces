// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! # PlexSpaces Channel
//!
//! ## Purpose
//! Provides an extensible channel/queue abstraction for microservices communication,
//! supporting both in-process (Go-like channels) and inter-process (Redis Streams, Kafka)
//! messaging patterns.
//!
//! ## Architecture Context
//! This crate is central to the PlexSpaces microservices framework, enabling:
//! - **Worker Queues**: Distribute work across elastic actor pools
//! - **Pub/Sub**: Event notification for GenEvent-style messaging
//! - **Request/Reply**: RPC-style communication patterns
//! - **Streaming**: Data pipelines with backpressure control
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │              PlexSpaces Microservices                │
//! ├─────────────────────────────────────────────────────┤
//! │                                                       │
//! │  ┌───────────┐     ┌─────────────┐     ┌─────────┐ │
//! │  │  Elastic  │────▶│   Channel   │────▶│ Circuit │ │
//! │  │   Pool    │     │ (This Crate)│     │ Breaker │ │
//! │  └───────────┘     └─────────────┘     └─────────┘ │
//! │        │                   │                  │      │
//! │        ▼                   ▼                  ▼      │
//! │  ┌────────────────────────────────────────────────┐ │
//! │  │           Channel Backends                     │ │
//! │  ├────────────────────────────────────────────────┤ │
//! │  │ InMemory  │  Redis Streams  │  Kafka  │  NATS │ │
//! │  │ (MPSC)    │  (Distributed)  │(Streaming)│(Pub/Sub)│ │
//! │  └────────────────────────────────────────────────┘ │
//! └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`Channel`]: Core trait for channel operations (send, receive, subscribe)
//! - [`InMemoryChannel`]: Go-like MPSC channel for same-node communication
//! - [`ChannelConfig`]: Configuration for channel creation
//! - [`ChannelMessage`]: Message envelope with metadata
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_proto`]: Protocol buffer definitions for channel.proto
//! - [`tokio`]: Async runtime for non-blocking operations
//! - [`futures`]: Stream combinators for channel operations
//!
//! ## Dependents
//! This crate is used by:
//! - `plexspaces-pool`: Elastic actor pools use channels for work distribution
//! - `plexspaces-registry`: Service registry uses pub/sub for events
//! - `plexspaces-circuitbreaker`: Circuit breaker uses channels for metrics
//!
//! ## Examples
//!
//! ### Basic Usage - InMemory Channel
//! ```rust
//! use plexspaces_channel::*;
//! use plexspaces_proto::plexspaces::channel::v1::*;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create in-memory channel with FIFO ordering
//! let config = ChannelConfig {
//!     name: "work-queue".to_string(),
//!     backend: ChannelBackend::ChannelBackendInMemory as i32,
//!     capacity: 100,
//!     delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
//!     ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
//!     ..Default::default()
//! };
//!
//! let channel = InMemoryChannel::new(config).await?;
//!
//! // Send message
//! let msg = ChannelMessage {
//!     id: ulid::Ulid::new().to_string(),
//!     channel: "work-queue".to_string(),
//!     payload: b"task data".to_vec(),
//!     ..Default::default()
//! };
//! channel.send(msg).await?;
//!
//! // Receive message
//! let received = channel.receive(1).await?;
//! assert_eq!(received.len(), 1);
//! # Ok(())
//! # }
//! ```
//!
//! ### Pub/Sub Pattern
//! ```rust
//! use plexspaces_channel::*;
//! use plexspaces_proto::plexspaces::channel::v1::*;
//! use futures::StreamExt;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = ChannelConfig {
//!     name: "events".to_string(),
//!     backend: ChannelBackend::ChannelBackendInMemory as i32,
//!     capacity: 0, // Unbounded for pub/sub
//!     delivery: DeliveryGuarantee::DeliveryGuaranteeAtMostOnce as i32,
//!     ..Default::default()
//! };
//!
//! let channel = InMemoryChannel::new(config).await?;
//!
//! // Subscribe to events
//! let mut stream = channel.subscribe(None).await?;
//!
//! // Publish event (in another task)
//! let msg = ChannelMessage {
//!     id: ulid::Ulid::new().to_string(),
//!     channel: "events".to_string(),
//!     payload: b"event occurred".to_vec(),
//!     ..Default::default()
//! };
//! channel.publish(msg).await?;
//!
//! // Receive from subscription
//! if let Some(event) = stream.next().await {
//!     println!("Received event: {:?}", event);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All channel operations are defined in `proto/plexspaces/v1/channel/channel.proto`.
//! This crate implements the `ChannelService` gRPC service and provides Rust abstractions.
//!
//! ### Extensibility
//! Multiple backend implementations:
//! - **InMemory**: Fast, same-node, Go-like MPSC channels
//! - **Redis**: Distributed, persistent, Redis Streams with consumer groups
//! - **Kafka**: High-throughput, partitioned, replicated streaming
//! - **NATS**: Lightweight distributed pub/sub with queue groups
//! - **Custom**: User-provided implementations via `Channel` trait
//!
//! ### Static vs Dynamic
//! - **Core**: Channel trait and message types (static, always present)
//! - **Backends**: Pluggable implementations (dynamic, feature-gated)
//! - **Configuration**: Runtime-selectable backend via `ChannelConfig`
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-channel
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-channel
//! ```
//!
//! ## Performance Characteristics
//! - **InMemory**: < 10μs latency, > 1M msgs/sec throughput
//! - **Redis**: < 1ms latency, > 100K msgs/sec throughput
//! - **Kafka**: < 5ms latency, > 1M msgs/sec throughput (batched)
//!
//! ## Known Limitations
//! - Redis backend requires Redis server (feature-gated)
//! - Kafka backend requires Kafka cluster (feature-gated)
//! - NATS backend requires NATS server (feature-gated)
//! - InMemory channel is not persistent (messages lost on restart)
//! - Latency and throughput metrics not yet implemented (TODOs in code)
//! - Dead Letter Queue (DLQ) not fully implemented across all backends

#![warn(missing_docs)]
#![warn(clippy::all)]

mod channel;
mod in_memory;

#[cfg(feature = "redis-backend")]
mod redis_backend;

#[cfg(feature = "kafka-backend")]
mod kafka_backend;

#[cfg(feature = "nats-backend")]
mod nats_backend;

#[cfg(feature = "sqlite-backend")]
mod sqlite_backend;

// Re-export all public items
pub use channel::*;
pub use in_memory::*;

#[cfg(feature = "redis-backend")]
pub use redis_backend::*;

#[cfg(feature = "kafka-backend")]
pub use kafka_backend::*;

#[cfg(feature = "nats-backend")]
pub use nats_backend::*;

#[cfg(feature = "sqlite-backend")]
pub use sqlite_backend::*;
