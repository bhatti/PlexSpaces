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

//! # PlexSpaces Application Module
//!
//! ## Purpose
//! Defines the Application trait and lifecycle management for PlexSpaces applications
//! following Erlang/OTP application patterns.
//!
//! ## Architecture Context
//! Applications are the fundamental unit of code organization in PlexSpaces:
//! - Library applications provide reusable modules (no processes)
//! - Active applications have supervision trees and long-running processes
//! - Applications have dependencies (topological startup order)
//! - Applications have lifecycle hooks (start, stop)
//!
//! ## Proto-First Design
//! Uses proto-generated types from `plexspaces_proto::node::v1`:
//! - `ApplicationSpec` - Application configuration
//! - `ApplicationState` - Runtime state
//! - `ApplicationStatus` - Lifecycle status
//!
//! ## Examples
//!
//! ### Implementing an Application
//! ```rust,no_run
//! use plexspaces::application::{Application, ApplicationContext};
//! use async_trait::async_trait;
//!
//! struct MyApp;
//!
//! #[async_trait]
//! impl Application for MyApp {
//!     async fn start(&self, ctx: &ApplicationContext) -> Result<(), Box<dyn std::error::Error>> {
//!         // Start application logic
//!         Ok(())
//!     }
//!
//!     async fn stop(&self, ctx: &ApplicationContext) -> Result<(), Box<dyn std::error::Error>> {
//!         // Stop application logic
//!         Ok(())
//!     }
//! }
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use thiserror::Error;

// Import proto-generated types
pub use plexspaces_proto::application::v1::{
    ApplicationRuntimeState, ApplicationSpec, ApplicationStatus, ApplicationType,
};
pub use plexspaces_proto::node::v1::{
    ApplicationConfig,
};

// Sub-modules
pub mod controller;
pub mod supervisor_builder;

// Re-export controller and supervisor builder
pub use controller::ApplicationController;
pub use supervisor_builder::SupervisorBuilder;

/// Application errors
#[derive(Debug, Error)]
pub enum ApplicationError {
    /// Failed to start application
    #[error("Failed to start application: {0}")]
    StartFailed(String),

    /// Failed to stop application
    #[error("Failed to stop application: {0}")]
    StopFailed(String),

    /// Application not found
    #[error("Application not found: {0}")]
    NotFound(String),

    /// Invalid state transition
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition {
        from: ApplicationStatus,
        to: ApplicationStatus,
    },

    /// Dependency not satisfied
    #[error("Dependency not satisfied: {0}")]
    DependencyNotSatisfied(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Application context passed to lifecycle hooks
///
/// ## Purpose
/// Provides runtime context to applications during lifecycle operations.
/// Uses proto ApplicationState for runtime state.
///
/// ## Design
/// - Proto-First: Wraps ApplicationState from proto
/// - Provides convenience methods for common operations
pub struct ApplicationContext {
    /// Application state (proto type)
    state: ApplicationRuntimeState,
}

impl ApplicationContext {
    /// Create new application context from state
    ///
    /// ## Arguments
    /// * `state` - Proto ApplicationRuntimeState
    pub fn new(state: ApplicationRuntimeState) -> Self {
        Self { state }
    }

    /// Create from config and env (convenience)
    ///
    /// ## Arguments
    /// * `config` - Application configuration
    /// * `env` - Environment variables
    pub fn from_config(config: ApplicationConfig, env: HashMap<String, String>) -> Self {
        let state = ApplicationRuntimeState {
            name: config.name.clone(),
            status: ApplicationStatus::ApplicationStatusLoading.into(),
            start_timestamp_ms: 0,
            supervisor_pid: None,
            env,
        };
        Self::new(state)
    }

    /// Get environment variable
    pub fn get_env(&self, key: &str) -> Option<&String> {
        self.state.env.get(key)
    }

    /// Get application name
    pub fn name(&self) -> &str {
        &self.state.name
    }

    /// Get application state (proto)
    pub fn state(&self) -> &ApplicationRuntimeState {
        &self.state
    }

    /// Get application status
    pub fn status(&self) -> ApplicationStatus {
        ApplicationStatus::try_from(self.state.status as i32).unwrap_or(ApplicationStatus::ApplicationStatusUnspecified)
    }
}

/// Application trait for lifecycle management
///
/// ## Purpose
/// Defines the interface that all PlexSpaces applications must implement.
/// Follows Erlang/OTP application behavior pattern.
///
/// ## Lifecycle
/// 1. `start()` - Initialize application, start processes
/// 2. Application runs
/// 3. `stop()` - Gracefully shutdown application
///
/// ## Design Notes
/// - Library applications can provide no-op implementations
/// - Active applications start supervision trees in `start()`
/// - Applications should be idempotent (multiple start/stop calls safe)
#[async_trait]
pub trait Application: Send + Sync {
    /// Start the application
    ///
    /// ## Arguments
    /// * `ctx` - Application context with configuration and environment
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::StartFailed` if startup fails
    ///
    /// ## Design Notes
    /// - Should be idempotent (calling twice is safe)
    /// - Should start supervision tree if active application
    /// - Should register services, initialize state
    async fn start(&self, ctx: &ApplicationContext) -> Result<(), ApplicationError>;

    /// Stop the application
    ///
    /// ## Arguments
    /// * `ctx` - Application context
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::StopFailed` if shutdown fails
    ///
    /// ## Design Notes
    /// - Should be idempotent (calling twice is safe)
    /// - Should stop supervision tree gracefully
    /// - Should deregister services, cleanup state
    async fn stop(&self, ctx: &ApplicationContext) -> Result<(), ApplicationError>;

    /// Get application name
    ///
    /// ## Returns
    /// Application name string
    fn name(&self) -> &str;

    /// Get application type
    ///
    /// ## Returns
    /// Application type (Library or Active)
    fn app_type(&self) -> ApplicationType {
        ApplicationType::ApplicationTypeLibrary
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock application for testing
    struct MockApplication {
        name: String,
        start_called: Arc<RwLock<bool>>,
        stop_called: Arc<RwLock<bool>>,
    }

    impl MockApplication {
        fn new(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                start_called: Arc::new(RwLock::new(false)),
                stop_called: Arc::new(RwLock::new(false)),
            }
        }

        async fn was_start_called(&self) -> bool {
            *self.start_called.read().await
        }

        async fn was_stop_called(&self) -> bool {
            *self.stop_called.read().await
        }
    }

    #[async_trait]
    impl Application for MockApplication {
        async fn start(&self, _ctx: &ApplicationContext) -> Result<(), ApplicationError> {
            let mut started = self.start_called.write().await;
            *started = true;
            Ok(())
        }

        async fn stop(&self, _ctx: &ApplicationContext) -> Result<(), ApplicationError> {
            let mut stopped = self.stop_called.write().await;
            *stopped = true;
            Ok(())
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    /// Test: Create application context
    #[test]
    fn test_application_context_creation() {
        let config = ApplicationConfig {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            config_path: "test.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        let mut env = HashMap::new();
        env.insert("LOG_LEVEL".to_string(), "info".to_string());

        let ctx = ApplicationContext::from_config(config, env);

        assert_eq!(ctx.name(), "test-app");
        assert_eq!(ctx.get_env("LOG_LEVEL"), Some(&"info".to_string()));
    }

    /// Test: Application start lifecycle
    #[tokio::test]
    async fn test_application_start() {
        let app = MockApplication::new("test-app");

        let config = ApplicationConfig {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            config_path: "test.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        let ctx = ApplicationContext::from_config(config, HashMap::new());

        // Start should succeed
        app.start(&ctx).await.expect("Start should succeed");

        // Verify start was called
        assert!(app.was_start_called().await);
        assert!(!app.was_stop_called().await);
    }

    /// Test: Application stop lifecycle
    #[tokio::test]
    async fn test_application_stop() {
        let app = MockApplication::new("test-app");

        let config = ApplicationConfig {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            config_path: "test.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        let ctx = ApplicationContext::from_config(config, HashMap::new());

        // Start first
        app.start(&ctx).await.expect("Start should succeed");

        // Then stop
        app.stop(&ctx).await.expect("Stop should succeed");

        // Verify both were called
        assert!(app.was_start_called().await);
        assert!(app.was_stop_called().await);
    }

    /// Test: Application name
    #[test]
    fn test_application_name() {
        let app = MockApplication::new("my-app");
        assert_eq!(app.name(), "my-app");
    }

    /// Test: Application type default
    #[test]
    fn test_application_type_default() {
        let app = MockApplication::new("test-app");
        assert_eq!(app.app_type(), ApplicationType::ApplicationTypeLibrary);
    }

    /// Test: Environment variable access
    #[test]
    fn test_environment_variable_access() {
        let config = ApplicationConfig {
            name: "test-app".to_string(),
            version: "1.0.0".to_string(),
            config_path: "test.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        let mut env = HashMap::new();
        env.insert("KEY1".to_string(), "value1".to_string());
        env.insert("KEY2".to_string(), "value2".to_string());

        let ctx = ApplicationContext::from_config(config, env);

        assert_eq!(ctx.get_env("KEY1"), Some(&"value1".to_string()));
        assert_eq!(ctx.get_env("KEY2"), Some(&"value2".to_string()));
        assert_eq!(ctx.get_env("KEY3"), None);
    }
}
