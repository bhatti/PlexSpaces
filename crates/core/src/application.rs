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

//! # PlexSpaces Application Trait
//!
//! ## Purpose
//! Provides the `Application` trait for implementing business logic layers that run on PlexSpaces nodes.
//! Follows Erlang/OTP application behavior pattern while optimizing for containerized deployments.
//!
//! ## Architecture Context
//! In PlexSpaces (following Erlang/OTP principles):
//! - **Node** = Infrastructure layer (gRPC, actor registry, remoting, health checks)
//! - **Application** = Business logic layer (supervision trees, workers, domain actors)
//! - **Release** = Docker image with Node binary as entry point
//!
//! See: `docs/NODE_APPLICATION_ARCHITECTURE.md` for full architecture documentation.
//!
//! ## Design Principles
//! 1. **Proto-First**: Data models defined in `proto/plexspaces/v1/application/application.proto`
//! 2. **Trait in Rust**: Application trait defines behavior (Rust-specific)
//! 3. **One app per node**: Default to single application per container
//! 4. **Clear separation**: Node provides infrastructure, Application implements logic
//!
//! ## Examples
//!
//! ### Basic Application Implementation
//! ```rust
//! use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
//! use plexspaces_proto::v1::application::{ApplicationConfig, HealthStatus};
//! use async_trait::async_trait;
//! use std::sync::Arc;
//!
//! pub struct MyApplication {
//!     config: ApplicationConfig,
//! }
//!
//! #[async_trait]
//! impl Application for MyApplication {
//!     fn name(&self) -> &str {
//!         &self.config.name
//!     }
//!
//!     fn version(&self) -> &str {
//!         &self.config.version
//!     }
//!
//!     async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
//!         // Spawn supervision tree, workers, etc.
//!         Ok(())
//!     }
//!
//!     async fn stop(&mut self) -> Result<(), ApplicationError> {
//!         // Graceful shutdown: drain work, save state
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use std::sync::Arc;
use thiserror::Error;

// Re-export proto types for convenience
pub use plexspaces_proto::v1::application::{
    ApplicationConfig, ApplicationState, ApplicationStatistics, HealthStatus, ShutdownStrategy,
};

/// Application trait for implementing business logic layers
///
/// ## Purpose
/// Defines the lifecycle and behavior of a PlexSpaces application (business logic layer).
/// Applications implement domain-specific logic using the infrastructure provided by the Node.
///
/// ## Lifecycle
/// 1. **Creation**: Application struct instantiated with config and environment
/// 2. **Start**: `start()` called, application spawns supervision tree and actors
/// 3. **Running**: Application processes work via actors
/// 4. **Stop**: `stop()` called, application drains work and saves state
///
/// ## Design Notes
/// - Applications receive a reference to `ApplicationNode` (minimal Node interface)
/// - Applications are responsible for their own supervision trees
/// - Applications should implement graceful shutdown (drain in-flight work)
/// - Applications can be tested with mock `ApplicationNode` implementations
///
/// ## Configuration
/// Application configuration is loaded from TOML and parsed into `ApplicationConfig` proto message.
#[async_trait]
pub trait Application: Send + Sync {
    /// Application name (must match TOML [[applications]].name)
    ///
    /// ## Examples
    /// - "genomics-coordinator"
    /// - "finance-risk-workers"
    fn name(&self) -> &str;

    /// Application version (semantic versioning)
    ///
    /// ## Examples
    /// - "0.1.0"
    /// - "1.2.3"
    fn version(&self) -> &str;

    /// Start the application
    ///
    /// ## Purpose
    /// Initialize application resources and spawn actors/supervision trees.
    ///
    /// ## Arguments
    /// * `node` - Reference to minimal Node interface providing infrastructure
    ///
    /// ## Returns
    /// * `Ok(())` - Application started successfully
    /// * `Err(ApplicationError)` - Startup failed
    ///
    /// ## Typical Implementation
    /// 1. Create root supervision tree
    /// 2. Spawn worker pools based on environment config
    /// 3. Initialize application-specific resources
    /// 4. Register actors with node
    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError>;

    /// Stop the application gracefully
    ///
    /// ## Purpose
    /// Perform graceful shutdown: drain in-flight work, save state, cleanup resources.
    ///
    /// ## Returns
    /// * `Ok(())` - Application stopped successfully
    /// * `Err(ApplicationError)` - Shutdown failed
    ///
    /// ## Typical Implementation
    /// 1. Stop accepting new work
    /// 2. Drain in-flight messages/tasks
    /// 3. Save durable state (if applicable)
    /// 4. Stop supervision tree and actors
    /// 5. Disconnect from external services
    ///
    /// ## Timeout
    /// Applications have `shutdown_timeout` from `ApplicationConfig` to complete.
    /// If exceeded, node may force kill the application.
    async fn stop(&mut self) -> Result<(), ApplicationError>;

    /// Health check for application
    ///
    /// ## Purpose
    /// Report application health status for monitoring and readiness probes.
    ///
    /// ## Returns
    /// `HealthStatus` from proto (Healthy, Degraded, Unhealthy)
    ///
    /// ## Design Notes
    /// - Default implementation returns `Healthy`
    /// - Override for custom health checks
    /// - Called periodically by Node health service
    /// - Used for Kubernetes readiness/liveness probes
    async fn health_check(&self) -> HealthStatus {
        HealthStatus::HealthStatusHealthy
    }
    
    /// Get reference to Any for downcasting
    ///
    /// ## Purpose
    /// Allows downcasting to concrete application types (SpecApplication, WasmApplication).
    ///
    /// ## Returns
    /// Reference to `std::any::Any` for type checking and downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Minimal Node interface needed by Applications
///
/// ## Purpose
/// Defines the minimal infrastructure services that Applications need from the Node.
/// This trait allows applications to be tested with mock implementations.
///
/// ## Design Notes
/// - Applications should NOT access Node internals directly
/// - This trait provides a stable interface for application development
/// - Mock implementations can be used for testing applications in isolation
#[async_trait]
pub trait ApplicationNode: Send + Sync {
    /// Get node ID
    fn id(&self) -> &str;

    /// Get node listen address (gRPC server address)
    fn listen_addr(&self) -> &str;

    /// Get ServiceLocator (optional - only Node implements this)
    ///
    /// ## Purpose
    /// Allows applications to access ServiceLocator for ActorFactory.
    /// Node implementations return Some(service_locator), mocks return None.
    ///
    /// ## Returns
    /// Some(ServiceLocator) if available, None otherwise
    fn service_locator(&self) -> Option<Arc<crate::ServiceLocator>> {
        None
    }
}

/// Application errors
///
/// ## Purpose
/// Errors that can occur during application lifecycle (start, stop, health checks).
///
/// ## Design Notes
/// - These are Rust-specific error types (not in proto)
/// - Proto defines `ApplicationState::ApplicationStateFailed` for state tracking
/// - Error messages can be logged and reported via proto `Application` message
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// Application startup failed
    #[error("Application startup failed: {0}")]
    StartupFailed(String),

    /// Application shutdown failed
    #[error("Application shutdown failed: {0}")]
    ShutdownFailed(String),

    /// Dependency not found or failed to start
    #[error("Application dependency failed: {0}")]
    DependencyFailed(String),

    /// Configuration error
    #[error("Application configuration error: {0}")]
    ConfigError(String),

    /// Shutdown timeout exceeded
    #[error("Application shutdown timeout exceeded ({0:?})")]
    ShutdownTimeout(prost_types::Duration),

    /// Actor spawn error
    #[error("Actor spawn failed for '{0}': {1}")]
    ActorSpawnFailed(String, String),

    /// Actor stop error
    #[error("Actor stop failed for '{0}': {1}")]
    ActorStopFailed(String, String),

    /// Generic application error
    #[error("Application error: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockNode {
        id: String,
        addr: String,
    }

    #[async_trait]
    impl ApplicationNode for MockNode {
        fn id(&self) -> &str {
            &self.id
        }

        fn listen_addr(&self) -> &str {
            &self.addr
        }
    }

    struct TestApplication {
        name: String,
        version: String,
    }

    #[async_trait]
    impl Application for TestApplication {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
            Ok(())
        }

        async fn stop(&mut self) -> Result<(), ApplicationError> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn test_application_trait() {
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        let mut app = TestApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
        };

        assert_eq!(app.name(), "test-app");
        assert_eq!(app.version(), "0.1.0");

        app.start(node.clone()).await.unwrap();
        assert_eq!(app.health_check().await, HealthStatus::HealthStatusHealthy);
        app.stop().await.unwrap();
    }

    #[test]
    fn test_application_error_display() {
        let err = ApplicationError::StartupFailed("test error".to_string());
        assert_eq!(err.to_string(), "Application startup failed: test error");

        let duration = prost_types::Duration {
            seconds: 60,
            nanos: 0,
        };
        let err = ApplicationError::ShutdownTimeout(duration);
        assert!(err.to_string().contains("shutdown timeout exceeded"));
    }
}
