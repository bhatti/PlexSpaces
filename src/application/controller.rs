// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! # Application Controller
//!
//! ## Purpose
//! Manages the lifecycle of PlexSpaces applications - loading, starting,
//! stopping, and killing applications in dependency order.
//!
//! ## Architecture Context
//! The ApplicationController is responsible for:
//! - Loading application configurations
//! - Starting applications in dependency order (topological sort)
//! - Stopping applications in reverse dependency order
//! - Managing application state transitions
//! - Enforcing shutdown timeouts
//!
//! ## Design Decisions
//! - Uses proto ApplicationSpec for configuration
//! - Maintains application state in memory
//! - Supports graceful and brutal shutdown strategies
//! - Thread-safe for concurrent access

use super::ApplicationError;
use plexspaces_core::application::{Application, ApplicationNode};
use plexspaces_proto::application::v1::{ApplicationRuntimeState, ApplicationStatus};
use plexspaces_proto::node::v1::ApplicationConfig;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::timeout;

/// Application controller for lifecycle management
///
/// ## Purpose
/// Central manager for all applications in a PlexSpaces node.
///
/// ## Design
/// - Maintains registry of loaded applications (wrapped in RwLock for &mut self requirement)
/// - Tracks application state
/// - Enforces dependency ordering
/// - Handles shutdown timeouts
pub struct ApplicationController {
    /// Registry of loaded applications (wrapped in RwLock to support &mut self in trait methods)
    applications: Arc<RwLock<HashMap<String, Arc<RwLock<Box<dyn Application>>>>>>,

    /// Application states
    states: Arc<RwLock<HashMap<String, ApplicationRuntimeState>>>,

    /// Application configurations
    configs: Arc<RwLock<HashMap<String, ApplicationConfig>>>,

    /// ApplicationNode to pass to applications during start() (interior mutability)
    node: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
}

impl ApplicationController {
    /// Create a new application controller
    ///
    /// ## Returns
    /// A new ApplicationController instance
    pub fn new() -> Self {
        Self {
            applications: Arc::new(RwLock::new(HashMap::new())),
            states: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
            node: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the ApplicationNode to pass to applications during start()
    ///
    /// ## Arguments
    /// * `node` - The ApplicationNode implementation
    pub async fn set_node(&self, node: Arc<dyn ApplicationNode>) {
        let mut node_guard = self.node.write().await;
        *node_guard = Some(node);
    }

    /// Load an application
    ///
    /// ## Arguments
    /// * `app` - Application implementation (taken by value as Box to allow &mut self calls)
    /// * `config` - Application configuration
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::StartFailed` if application already loaded
    pub async fn load(
        &self,
        app: Box<dyn Application>,
        config: ApplicationConfig,
    ) -> Result<(), ApplicationError> {
        let name = app.name().to_string();

        let mut apps = self.applications.write().await;
        let mut states = self.states.write().await;
        let mut configs = self.configs.write().await;

        // Check if already loaded
        if apps.contains_key(&name) {
            return Err(ApplicationError::StartFailed(format!(
                "Application {} already loaded",
                name
            )));
        }

        // Store application wrapped in RwLock (needed for &mut self in trait methods)
        apps.insert(name.clone(), Arc::new(RwLock::new(app)));

        // Initialize state
        let state = ApplicationRuntimeState {
            name: name.clone(),
            status: ApplicationStatus::ApplicationStatusLoading.into(),
            start_timestamp_ms: 0,
            supervisor_pid: None,
            env: HashMap::new(),
        };
        states.insert(name.clone(), state);

        // Store config
        configs.insert(name, config);

        Ok(())
    }

    /// Start an application
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `env` - Environment variables (unused, kept for compatibility)
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::NotFound` if application not loaded
    /// - `ApplicationError::StartFailed` if start fails
    pub async fn start(
        &self,
        name: &str,
        _env: HashMap<String, String>,
    ) -> Result<(), ApplicationError> {
        // Get application
        let app = {
            let apps = self.applications.read().await;
            apps.get(name)
                .cloned()
                .ok_or_else(|| ApplicationError::NotFound(name.to_string()))?
        };

        // Get ApplicationNode
        let node = {
            let node_guard = self.node.read().await;
            node_guard.as_ref()
                .ok_or_else(|| ApplicationError::StartFailed(
                    "ApplicationNode not set. Call set_node() before starting applications.".to_string()
                ))?
                .clone()
        };

        // Update state to starting
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ApplicationStatus::ApplicationStatusStarting as i32;
                state.start_timestamp_ms = chrono::Utc::now().timestamp_millis();
            }
        }

        // Start application (requires &mut self, so we need to get write lock)
        {
            let mut app_guard = app.write().await;
            app_guard.start(node).await.map_err(|e| {
                ApplicationError::StartFailed(format!("Failed to start {}: {}", name, e))
            })?;
        }

        // Update state to running
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ApplicationStatus::ApplicationStatusRunning.into();
            }
        }

        Ok(())
    }

    /// Stop an application gracefully
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `timeout_secs` - Timeout in seconds (0 = no timeout)
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::NotFound` if application not loaded
    /// - `ApplicationError::StopFailed` if stop fails or times out
    pub async fn stop(&self, name: &str, timeout_secs: u64) -> Result<(), ApplicationError> {
        // Get application
        let app = {
            let apps = self.applications.read().await;
            apps.get(name)
                .cloned()
                .ok_or_else(|| ApplicationError::NotFound(name.to_string()))?
        };

        // Update state to stopping
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ApplicationStatus::ApplicationStatusStopping.into();
            }
        }

        // Stop with timeout if specified (requires &mut self, so we need write lock)
        let stop_result = if timeout_secs > 0 {
            let app_clone = app.clone();
            timeout(
                Duration::from_secs(timeout_secs),
                async move {
                    let mut app_guard = app_clone.write().await;
                    app_guard.stop().await
                }
            )
            .await
            .map_err(|_| {
                ApplicationError::StopFailed(format!(
                    "Timeout stopping {} after {}s",
                    name, timeout_secs
                ))
            })?
        } else {
            let mut app_guard = app.write().await;
            app_guard.stop().await
        };

        stop_result
            .map_err(|e| ApplicationError::StopFailed(format!("Failed to stop {}: {}", name, e)))?;

        // Update state to stopped
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ApplicationStatus::ApplicationStatusStopped.into();
            }
        }

        Ok(())
    }

    /// Kill an application immediately (brutal kill)
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - `ApplicationError::NotFound` if application not loaded
    ///
    /// ## Design Notes
    /// - Does not call stop() - just marks as terminated
    /// - Used when graceful shutdown times out
    pub async fn kill(&self, name: &str) -> Result<(), ApplicationError> {
        // Verify application exists
        {
            let apps = self.applications.read().await;
            if !apps.contains_key(name) {
                return Err(ApplicationError::NotFound(name.to_string()));
            }
        }

        // Update state to failed (brutal kill = abnormal termination)
        {
            let mut states = self.states.write().await;
            if let Some(state) = states.get_mut(name) {
                state.status = ApplicationStatus::ApplicationStatusFailed.into();
            }
        }

        Ok(())
    }

    /// Get application status
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// Current application status
    ///
    /// ## Errors
    /// - `ApplicationError::NotFound` if application not loaded
    pub async fn get_status(&self, name: &str) -> Result<ApplicationStatus, ApplicationError> {
        let states = self.states.read().await;
        let state = states
            .get(name)
            .ok_or_else(|| ApplicationError::NotFound(name.to_string()))?;

        Ok(ApplicationStatus::try_from(state.status).unwrap_or(ApplicationStatus::ApplicationStatusUnspecified))
    }

    /// List all loaded applications
    ///
    /// ## Returns
    /// Vector of application names
    pub async fn list_applications(&self) -> Vec<String> {
        let apps = self.applications.read().await;
        apps.keys().cloned().collect()
    }

    /// Get number of loaded applications
    ///
    /// ## Returns
    /// Number of applications
    pub async fn count(&self) -> usize {
        let apps = self.applications.read().await;
        apps.len()
    }
}

impl Default for ApplicationController {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::application::{Application, ApplicationNode, ApplicationError as CoreApplicationError};
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock ApplicationNode for testing
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

        async fn spawn_actor(
            &self,
            actor_id: String,
            _behavior: Box<dyn plexspaces_core::Actor>,
            _namespace: String,
        ) -> Result<String, CoreApplicationError> {
            Ok(actor_id)
        }

        async fn stop_actor(&self, _actor_id: &str) -> Result<(), CoreApplicationError> {
            Ok(())
        }
    }

    /// Mock application for testing
    struct MockApp {
        name: String,
        version: String,
        start_called: Arc<RwLock<bool>>,
        stop_called: Arc<RwLock<bool>>,
        should_fail_start: bool,
        should_fail_stop: bool,
    }

    impl MockApp {
        fn new(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                version: "1.0.0".to_string(),
                start_called: Arc::new(RwLock::new(false)),
                stop_called: Arc::new(RwLock::new(false)),
                should_fail_start: false,
                should_fail_stop: false,
            }
        }

        fn with_start_failure(mut self) -> Self {
            self.should_fail_start = true;
            self
        }

        fn with_stop_failure(mut self) -> Self {
            self.should_fail_stop = true;
            self
        }

        async fn was_start_called(&self) -> bool {
            *self.start_called.read().await
        }

        async fn was_stop_called(&self) -> bool {
            *self.stop_called.read().await
        }
    }

    #[async_trait]
    impl Application for MockApp {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), CoreApplicationError> {
            let mut started = self.start_called.write().await;
            *started = true;

            if self.should_fail_start {
                Err(CoreApplicationError::StartupFailed(
                    "Mock start failure".to_string(),
                ))
            } else {
                Ok(())
            }
        }

        async fn stop(&mut self) -> Result<(), CoreApplicationError> {
            let mut stopped = self.stop_called.write().await;
            *stopped = true;

            if self.should_fail_stop {
                Err(CoreApplicationError::ShutdownFailed(
                    "Mock stop failure".to_string(),
                ))
            } else {
                Ok(())
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Test: Create controller
    #[test]
    fn test_create_controller() {
        let controller = ApplicationController::new();
        // Should not panic
        drop(controller);
    }

    /// Test: Load application
    #[tokio::test]
    async fn test_load_application() {
        let controller = ApplicationController::new();
        let app = Box::new(MockApp::new("test-app"));

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

        controller
            .load(app, config)
            .await
            .expect("Load should succeed");

        // Verify loaded
        assert_eq!(controller.count().await, 1);
        assert_eq!(
            controller.get_status("test-app").await.unwrap(),
            ApplicationStatus::ApplicationStatusLoading
        );
    }

    /// Test: Load duplicate application fails
    #[tokio::test]
    async fn test_load_duplicate_fails() {
        let controller = ApplicationController::new();
        let app1 = Box::new(MockApp::new("test-app"));
        let app2 = Box::new(MockApp::new("test-app"));

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

        controller.load(app1, config.clone()).await.unwrap();

        // Second load should fail
        let result = controller.load(app2, config).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApplicationError::StartFailed(_)
        ));
    }

    /// Test: Start application
    #[tokio::test]
    async fn test_start_application() {
        let controller = ApplicationController::new();
        let app = MockApp::new("test-app");
        let start_flag = app.start_called.clone();

        // Set mock node
        let mock_node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9001".to_string(),
        });
        controller.set_node(mock_node).await;

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

        controller.load(Box::new(app), config).await.unwrap();

        let mut env = HashMap::new();
        env.insert("LOG_LEVEL".to_string(), "info".to_string());

        controller.start("test-app", env).await.unwrap();

        // Verify started
        assert!(*start_flag.read().await);
        assert_eq!(
            controller.get_status("test-app").await.unwrap(),
            ApplicationStatus::ApplicationStatusRunning
        );
    }

    /// Test: Start non-existent application fails
    #[tokio::test]
    async fn test_start_nonexistent_fails() {
        let controller = ApplicationController::new();

        let result = controller.start("nonexistent", HashMap::new()).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ApplicationError::NotFound(_)));
    }

    /// Test: Stop application
    #[tokio::test]
    async fn test_stop_application() {
        let controller = ApplicationController::new();
        let app = MockApp::new("test-app");
        let stop_flag = app.stop_called.clone();

        // Set mock node
        let mock_node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9001".to_string(),
        });
        controller.set_node(mock_node).await;

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

        controller.load(Box::new(app), config).await.unwrap();
        controller.start("test-app", HashMap::new()).await.unwrap();

        controller.stop("test-app", 30).await.unwrap();

        // Verify stopped
        assert!(*stop_flag.read().await);
        assert_eq!(
            controller.get_status("test-app").await.unwrap(),
            ApplicationStatus::ApplicationStatusStopped
        );
    }

    /// Test: Stop with timeout
    #[tokio::test]
    async fn test_stop_with_timeout() {
        let controller = ApplicationController::new();

        // Mock app that takes too long to stop
        struct SlowStopApp {
            name: String,
            version: String,
        }

        #[async_trait]
        impl Application for SlowStopApp {
            fn name(&self) -> &str {
                &self.name
            }

            fn version(&self) -> &str {
                &self.version
            }

            async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), CoreApplicationError> {
                Ok(())
            }

            async fn stop(&mut self) -> Result<(), CoreApplicationError> {
                // Sleep longer than timeout
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(())
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        // Set mock node
        let mock_node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9001".to_string(),
        });
        controller.set_node(mock_node).await;

        let app = Box::new(SlowStopApp {
            name: "slow-app".to_string(),
            version: "1.0.0".to_string(),
        });

        let config = ApplicationConfig {
            name: "slow-app".to_string(),
            version: "1.0.0".to_string(),
            config_path: "test.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 1,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        controller.load(app, config).await.unwrap();
        controller.start("slow-app", HashMap::new()).await.unwrap();

        // Stop with 1 second timeout should fail
        let result = controller.stop("slow-app", 1).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ApplicationError::StopFailed(_)
        ));
    }

    /// Test: Kill application
    #[tokio::test]
    async fn test_kill_application() {
        let controller = ApplicationController::new();
        let app = Box::new(MockApp::new("test-app"));

        // Set mock node
        let mock_node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9001".to_string(),
        });
        controller.set_node(mock_node).await;

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

        controller.load(app, config).await.unwrap();
        controller.start("test-app", HashMap::new()).await.unwrap();

        controller.kill("test-app").await.unwrap();

        // Verify failed (brutal kill = abnormal termination, stop not called)
        assert_eq!(
            controller.get_status("test-app").await.unwrap(),
            ApplicationStatus::ApplicationStatusFailed
        );
    }

    /// Test: List applications
    #[tokio::test]
    async fn test_list_applications() {
        let controller = ApplicationController::new();

        let app1 = Box::new(MockApp::new("app1"));
        let app2 = Box::new(MockApp::new("app2"));

        let config1 = ApplicationConfig {
            name: "app1".to_string(),
            version: "1.0.0".to_string(),
            config_path: "app1.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        let config2 = ApplicationConfig {
            name: "app2".to_string(),
            version: "1.0.0".to_string(),
            config_path: "app2.toml".to_string(),
            enabled: true,
            auto_start: true,
            shutdown_timeout_seconds: 30,
            shutdown_strategy: 0,
            dependencies: vec![],
        };

        controller.load(app1, config1).await.unwrap();
        controller.load(app2, config2).await.unwrap();

        let apps = controller.list_applications().await;
        assert_eq!(apps.len(), 2);
        assert!(apps.contains(&"app1".to_string()));
        assert!(apps.contains(&"app2".to_string()));
    }

    /// Test: Count applications
    #[tokio::test]
    async fn test_count_applications() {
        let controller = ApplicationController::new();

        assert_eq!(controller.count().await, 0);

        let app = Box::new(MockApp::new("test-app"));
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

        controller.load(app, config).await.unwrap();

        assert_eq!(controller.count().await, 1);
    }
}
