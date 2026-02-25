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

//! Service Lifecycle trait for ordered startup/shutdown
//!
//! ## Purpose
//! Provides lifecycle hooks for services that need ordered startup and shutdown.
//! Services register with ServiceLocator and participate in the node's lifecycle.
//!
//! ## Startup Order
//! Services are started in order of `startup_priority()`:
//! 1. Infrastructure (10-50): DB, KV store, lock manager
//! 2. Core services (50-100): Actor registry, object registry
//! 3. Application services (100+): Application manager, HTTP gateway
//!
//! ## Shutdown Order
//! Services are stopped in order of `shutdown_priority()`:
//! 1. Application services (10-50): Stop accepting new requests
//! 2. Core services (50-100): Drain in-progress work
//! 3. Infrastructure (100+): Close connections, flush data

use async_trait::async_trait;

/// Lifecycle hooks for services that need ordered startup/shutdown.
///
/// Services that implement this trait participate in the node's
/// ordered startup and graceful shutdown sequences.
///
/// ## Example
/// ```rust,ignore
/// struct MyService { ... }
///
/// #[async_trait]
/// impl ServiceLifecycle for MyService {
///     async fn on_start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         // Initialize connections, start background tasks
///         Ok(())
///     }
///
///     async fn on_stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
///         // Stop accepting work, drain in-progress tasks
///         Ok(())
///     }
///
///     fn startup_priority(&self) -> u32 { 50 }  // Start after infrastructure
///     fn shutdown_priority(&self) -> u32 { 50 }  // Stop before infrastructure
/// }
/// ```
#[async_trait]
pub trait ServiceLifecycle: Send + Sync {
    /// Called during node startup, after all services are registered.
    ///
    /// Use this for initialization that depends on other services being available.
    /// Called in order of `startup_priority()` (lower values first).
    ///
    /// ## Guarantees
    /// - Called once during node startup
    /// - All services with lower priority have already started
    /// - ServiceLocator is fully populated before any on_start() is called
    async fn on_start(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Called during graceful shutdown, before services are deregistered.
    ///
    /// Services should stop accepting new work but finish in-progress operations.
    /// Called in order of `shutdown_priority()` (lower values first).
    ///
    /// ## Guarantees
    /// - Called once during shutdown
    /// - All services with lower priority have already stopped
    /// - In-progress operations should be given time to complete
    async fn on_stop(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    /// Return startup priority (lower = starts first, higher = starts later).
    ///
    /// ## Suggested Ranges
    /// - 10-30: Infrastructure (DB, KV store, lock manager)
    /// - 30-50: Core registries (object registry, actor registry)
    /// - 50-80: Core services (actor service, channel service)
    /// - 80-100: Application services (application manager)
    /// - 100+: External-facing services (HTTP gateway, gRPC server)
    fn startup_priority(&self) -> u32 {
        100
    }

    /// Return shutdown priority (lower = stops first, higher = stops later).
    ///
    /// Typically the reverse of startup order: application services stop first,
    /// infrastructure stops last to allow draining of in-progress work.
    ///
    /// ## Suggested Ranges
    /// - 10-30: External-facing services (stop accepting requests)
    /// - 30-50: Application services (drain application work)
    /// - 50-80: Core services (drain actor messages)
    /// - 80-100: Core registries (deregister)
    /// - 100+: Infrastructure (close connections, flush data)
    fn shutdown_priority(&self) -> u32 {
        100
    }

    /// Service name for logging and diagnostics
    fn lifecycle_name(&self) -> &str {
        "unnamed-service"
    }
}
