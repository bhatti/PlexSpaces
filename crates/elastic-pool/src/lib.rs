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

//! # PlexSpaces Elastic Pool
//!
//! ## Purpose
//! Provides auto-scaling worker pools for microservices, inspired by Erlang poolboy
//! and database connection pools. Combines actors with dynamic scaling based on load.
//!
//! ## Architecture Context
//! ElasticPool is built on PlexSpaces core components:
//! - **Supervisor**: Manages worker actors with restart policies (from plexspaces-supervisor)
//! - **Channel**: Work queue for distributing requests (from plexspaces-channel)
//! - **Auto-scaler**: Monitors load and adjusts pool size dynamically
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    ElasticPool                               │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │            Auto-Scaler (Background Task)             │  │
//! │  │  - Monitors load (checkout queue, busy workers)       │  │
//! │  │  - Scales up when load > threshold (default: 0.8)     │  │
//! │  │  - Scales down when load < threshold (default: 0.3)   │  │
//! │  │  - Respects min/max pool size constraints             │  │
//! │  └─────────────┬────────────────────────────────────────┘  │
//! │                │  scale_up() / scale_down()                │
//! │                ▼                                            │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │          Supervisor (Worker Management)              │  │
//! │  │  - Spawns/stops worker actors                         │  │
//! │  │  - Restarts failed workers (OneForOne strategy)       │  │
//! │  │  - Monitors worker health                             │  │
//! │  └─────────────┬────────────────────────────────────────┘  │
//! │                │  manages workers                          │
//! │                ▼                                            │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │         Worker Pool (Actors)                          │  │
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐   │  │
//! │  │  │ Worker1 │ │ Worker2 │ │ Worker3 │ │ Worker4 │   │  │
//! │  │  │Available│ │  Busy   │ │Available│ │  Idle   │   │  │
//! │  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘   │  │
//! │  └─────────────┬────────────────────────────────────────┘  │
//! │                │  checkout / checkin                       │
//! │                ▼                                            │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │       Checkout Queue (Channel)                        │  │
//! │  │  - Waiters queue when all workers busy                │  │
//! │  │  - Fair scheduling (FIFO)                             │  │
//! │  │  - Timeout support                                    │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`ElasticPool`]: Main pool structure managing workers and auto-scaling
//! - [`PoolMetrics`]: Real-time metrics for monitoring and scaling decisions
//! - [`PoolConfig`]: Configuration for pool behavior and scaling policies
//! - [`ActorHandle`]: Handle for checked-out workers (returned to caller)
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_proto`]: Protocol buffer definitions (PoolConfig, PoolMetrics, etc.)
//! - [`plexspaces_channel`]: MPSC channel for checkout queue
//! - [`plexspaces_actor`]: Actor abstraction for workers
//! - [`plexspaces_supervisor`]: Supervisor for worker lifecycle management
//! - [`plexspaces_core`]: Common types and errors
//! - [`tokio`]: Async runtime for background tasks
//!
//! ## Dependents
//! This crate is used by:
//! - **Order-Processing Example**: Pool of payment service clients
//! - **Database Pools**: Manage expensive DB connections
//! - **HTTP Client Pools**: Reuse HTTP/2 connections
//! - **gRPC Client Pools**: Maintain open gRPC channels
//!
//! ## Examples
//!
//! ### Basic Usage (Database Connection Pool)
//! ```rust,no_run
//! # use plexspaces_elastic_pool::*;
//! # use plexspaces_proto::pool::v1::*;
//! # async fn example() -> Result<(), ElasticPoolError> {
//! // Configure pool for database connections
//! let config = PoolConfig {
//!     name: "db-pool".to_string(),
//!     min_size: 2,           // Always keep 2 connections
//!     max_size: 10,          // Never exceed 10 connections
//!     initial_size: 5,       // Start with 5 connections
//!     scaling_threshold: 0.8, // Scale up when 80% busy
//!     scale_down_threshold: 0.3, // Scale down when < 30% busy
//!     ..Default::default()
//! };
//!
//! // Create pool
//! let pool = ElasticPool::new(config).await?;
//!
//! // Checkout a worker (blocks if all busy, respects timeout)
//! let handle = pool.checkout(std::time::Duration::from_secs(5)).await?;
//!
//! // Use the worker
//! // ... perform database operations ...
//!
//! // Checkin when done
//! pool.checkin(handle).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Auto-Scaling Behavior
//! ```rust,no_run
//! # use plexspaces_elastic_pool::*;
//! # use plexspaces_proto::pool::v1::*;
//! # use plexspaces_proto::pool::v1::scaling_policy::Strategy;
//! # async fn example() -> Result<(), ElasticPoolError> {
//! // Configure aggressive scaling
//! let config = PoolConfig {
//!     name: "auto-scale-pool".to_string(),
//!     min_size: 5,
//!     max_size: 50,
//!     initial_size: 10,
//!     scaling_policy: Some(ScalingPolicy {
//!         strategy: Strategy::StrategyPercentage as i32, // Scale by percentage
//!         scale_factor: 0.5,          // Add/remove 50% of current size
//!         cooldown: Some(prost_types::Duration { seconds: 30, nanos: 0 }),
//!         min_scale_step: 2,          // Add/remove at least 2 workers
//!         max_scale_step: 10,         // Add/remove at most 10 workers
//!     }),
//!     scaling_check_interval: Some(prost_types::Duration { seconds: 10, nanos: 0 }),
//!     ..Default::default()
//! };
//!
//! let pool = ElasticPool::new(config).await?;
//!
//! // Pool auto-scales based on load:
//! // - High load (> 80%): Adds 50% more workers (up to max_scale_step=10)
//! // - Low load (< 30%): Removes 50% of workers (down to min_size)
//! // - Cooldown prevents thrashing (waits 30s after scaling)
//! # Ok(())
//! # }
//! ```
//!
//! ### Monitoring Pool Metrics
//! ```rust,no_run
//! # use plexspaces_elastic_pool::*;
//! # async fn example() -> Result<(), ElasticPoolError> {
//! # let pool = ElasticPool::new(Default::default()).await?;
//! // Get current pool statistics
//! let metrics = pool.get_metrics().await?;
//!
//! println!("Total workers: {}", metrics.total_actors);
//! println!("Busy workers: {}", metrics.busy_actors);
//! println!("Available workers: {}", metrics.available_actors);
//! println!("Current load: {:.2}%", metrics.current_load * 100.0);
//! println!("Avg checkout latency: {}μs", metrics.avg_checkout_latency);
//! println!("Waiting requests: {}", metrics.waiting_requests);
//! # Ok(())
//! # }
//! ```
//!
//! ### Manual Scaling (Override Auto-Scaler)
//! ```rust,no_run
//! # use plexspaces_elastic_pool::*;
//! # async fn example() -> Result<(), ElasticPoolError> {
//! # let pool = ElasticPool::new(Default::default()).await?;
//! // Pause auto-scaling
//! pool.pause_scaling().await?;
//!
//! // Manually scale to exact size
//! pool.scale_to(20).await?;
//!
//! // Or scale relatively (add/remove workers)
//! pool.scale_by(5).await?;  // Add 5 workers
//! pool.scale_by(-3).await?; // Remove 3 workers
//!
//! // Resume auto-scaling
//! pool.resume_scaling().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Graceful Shutdown
//! ```rust,no_run
//! # use plexspaces_elastic_pool::*;
//! # async fn example() -> Result<(), ElasticPoolError> {
//! # let pool = ElasticPool::new(Default::default()).await?;
//! // Drain pool (stop accepting new checkouts, wait for in-flight to finish)
//! pool.drain(std::time::Duration::from_secs(30)).await?;
//!
//! // Delete pool (stop all workers)
//! pool.delete(false).await?; // Wait for workers to finish
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All types come from `pool.proto` (PoolConfig, PoolMetrics, ScalingPolicy).
//! This ensures type safety, versioning, and language-agnostic interoperability.
//!
//! ### Inspired by Erlang poolboy
//! - Min/max pool sizes
//! - Overflow workers (temporary above min_size)
//! - Checkout timeout with fair queueing
//! - Health checks and worker replacement
//!
//! ### Auto-Scaling Strategies
//! - **Incremental**: Add/remove 1 worker at a time (conservative)
//! - **Percentage**: Add/remove X% of current size (balanced)
//! - **Exponential**: Double/halve pool size (aggressive)
//! - **Custom**: User-defined scaling function (advanced)
//!
//! ### Load Calculation
//! ```
//! load = (busy_workers + waiting_requests) / total_workers
//! ```
//! - load > 0.8 → Scale up
//! - load < 0.3 → Scale down
//! - Smoothed over time windows (1m, 5m) to prevent thrashing
//!
//! ### Cooldown Mechanism
//! After scaling event, wait cooldown period before next scaling decision.
//! This prevents rapid oscillation between scaling up/down.
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-elastic-pool
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-elastic-pool
//!
//! # Run examples
//! cargo run --example db_pool
//! ```
//!
//! ## Performance Characteristics
//! - **Checkout latency**: < 1ms when workers available, up to timeout when exhausted
//! - **Scaling latency**: < 100ms to spawn new workers
//! - **Throughput**: > 10K checkouts/sec (with available workers)
//! - **Memory**: ~1KB per idle worker, ~10KB per active worker
//! - **Scaling decisions**: Every 10s (configurable via scaling_check_interval)
//!
//! ## Known Limitations
//! - **No Watch Streaming Yet**: StreamMetrics RPC planned but not implemented
//! - **No Circuit Breaker Integration**: Planned for worker failure tracking
//! - **No Custom Scaling Functions**: Only built-in strategies supported
//! - **No Worker Affinity**: Round-robin distribution only (no sticky workers)

#![warn(missing_docs)]
#![warn(clippy::all)]

mod elastic_pool;

pub use elastic_pool::*;

// Re-export proto types for convenience
pub use plexspaces_proto::pool::v1::{
    pool_service_client::PoolServiceClient, pool_service_server::PoolService,
    pool_service_server::PoolServiceServer, ActorConfig, ActorHandle, CheckinRequest,
    CheckinResponse, CheckoutError, CheckoutRequest, CheckoutResponse, CreatePoolRequest,
    CreatePoolResponse, DeletePoolRequest, DeletePoolResponse, DrainRequest, DrainResponse,
    GetStatsRequest, GetStatsResponse, PauseScalingRequest, PauseScalingResponse, PoolConfig,
    PoolMetrics, ResumeScalingRequest, ResumeScalingResponse, ScaleRequest, ScaleResponse,
    ScalingPolicy, ScalingState, StreamMetricsRequest,
};
