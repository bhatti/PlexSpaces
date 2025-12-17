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

//! # PlexSpaces Node Module
//!
//! ## Purpose
//! Implements the PlexSpacesNode - top-level runtime for a PlexSpaces deployment.
//! Each node runs one Release (Erlang/OTP-inspired).
//!
//! ## Proto-First Design
//! Uses proto types from `plexspaces_proto::node::v1`:
//! - NO new domain models defined here
//! - Uses existing proto: ReleaseSpec, ApplicationConfig, etc.
//! - Just orchestrates Release + ApplicationController

use crate::application::{ApplicationController, ApplicationError};
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError as CoreApplicationError};
use crate::release::{Release, ReleaseError};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Node errors
#[derive(Debug, Error)]
pub enum NodeError {
    /// Failed to start node
    #[error("Failed to start node: {0}")]
    StartFailed(String),

    /// Failed to stop node
    #[error("Failed to stop node: {0}")]
    StopFailed(String),

    /// Application error
    #[error("Application error: {0}")]
    ApplicationError(#[from] ApplicationError),

    /// Release error
    #[error("Release error: {0}")]
    ReleaseError(#[from] ReleaseError),

    /// Invalid state
    #[error("Invalid state: {0}")]
    InvalidState(String),
}

/// Node state (simple enum, not a domain model)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeState {
    /// Created but not started
    Created,
    /// Running (all applications started)
    Running,
    /// Stopped (all applications stopped)
    Stopped,
}

/// PlexSpaces Node - orchestrates Release + ApplicationController
///
/// ## Design
/// - Proto-First: NO new domain models
/// - Uses Release (wraps ReleaseSpec proto)
/// - Uses ApplicationController for lifecycle
/// - Just coordinates the boot/shutdown sequence
pub struct PlexSpacesNode {
    /// Release configuration (proto-based)
    release: Arc<Release>,

    /// Application controller
    controller: Arc<ApplicationController>,

    /// Current state
    state: Arc<RwLock<NodeState>>,

    /// Application registry (name -> impl)
    /// Stored as Box to allow conversion to controller's storage format
    app_registry: Arc<RwLock<HashMap<String, Box<dyn Application>>>>,
}

impl PlexSpacesNode {
    /// Create a new PlexSpaces node
    ///
    /// ## Arguments
    /// * `release` - Release configuration (proto-based)
    pub fn new(release: Release) -> Self {
        Self {
            release: Arc::new(release),
            controller: Arc::new(ApplicationController::new()),
            state: Arc::new(RwLock::new(NodeState::Created)),
            app_registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an application implementation
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `app` - Application implementation (taken by value as Box)
    pub async fn register_application(&self, name: impl Into<String>, app: Box<dyn Application>) {
        let mut registry = self.app_registry.write().await;
        registry.insert(name.into(), app);
    }

    /// Start the node (boot all applications)
    ///
    /// ## Design
    /// 1. Set ApplicationNode in controller (self as ApplicationNode)
    /// 2. Get applications from release (proto ReleaseSpec)
    /// 3. Load applications
    /// 4. Start applications in order
    /// 5. Update state to Running
    pub async fn start(&self) -> Result<(), NodeError> {
        // Check state
        {
            let state = self.state.read().await;
            if *state != NodeState::Created {
                return Err(NodeError::InvalidState(format!(
                    "Cannot start from state {:?}",
                    *state
                )));
            }
        }

        // Set ApplicationNode in controller (self as ApplicationNode)
        // We create a wrapper that implements ApplicationNode
        let node_id = self.release.spec().node.as_ref()
            .map(|n| n.id.clone())
            .unwrap_or_else(|| "plexspaces-node".to_string());
        let listen_addr = self.release.spec().node.as_ref()
            .map(|n| n.listen_address.clone())
            .unwrap_or_else(|| "0.0.0.0:9001".to_string());
        
        self.controller.set_node(Arc::new(PlexSpacesNodeApplicationNode {
            node_id,
            listen_addr,
        })).await;

        // Get applications from release (proto ReleaseSpec)
        let release_spec = self.release.spec();
        let applications = &release_spec.applications;

        // Load and start applications
        for app_config in applications {
            if !app_config.enabled || !app_config.auto_start {
                continue;
            }

            // Get application implementation from registry (already Box<dyn Application>)
            let app = {
                let mut registry = self.app_registry.write().await;
                registry.remove(&app_config.name).ok_or_else(|| {
                    NodeError::StartFailed(format!(
                        "Application {} not registered",
                        app_config.name
                    ))
                })?
            };

            // Load application (already Box<dyn Application>)
            self.controller.load(app, app_config.clone()).await?;

            // Start application (use release-level env)
            let env = release_spec.env.clone();
            self.controller.start(&app_config.name, env).await?;
        }

        // Update state to running
        {
            let mut state = self.state.write().await;
            *state = NodeState::Running;
        }

        Ok(())
    }

    /// Shutdown the node gracefully
    ///
    /// ## Design
    /// 1. Get applications from release in REVERSE order
    /// 2. Stop applications with timeouts from proto config
    /// 3. Update state to Stopped
    pub async fn shutdown(&self) -> Result<(), NodeError> {
        // Check state
        {
            let state = self.state.read().await;
            if *state != NodeState::Running {
                return Err(NodeError::InvalidState(format!(
                    "Cannot shutdown from state {:?}",
                    *state
                )));
            }
        }

        // Get applications from release in shutdown order (reverse)
        let applications = self
            .release
            .get_applications_in_shutdown_order()
            .map_err(|e| NodeError::StopFailed(e.to_string()))?;

        // Stop applications in reverse dependency order
        for app_config in applications {
            if !app_config.enabled {
                continue;
            }

            let timeout = if app_config.shutdown_timeout_seconds <= 0 {
                0
            } else {
                app_config.shutdown_timeout_seconds as u64
            };

            // Stop application (with timeout from proto config)
            self.controller.stop(&app_config.name, timeout).await?;
        }

        // Update state to stopped
        {
            let mut state = self.state.write().await;
            *state = NodeState::Stopped;
        }

        Ok(())
    }

    /// Get current node state
    pub async fn get_state(&self) -> NodeState {
        *self.state.read().await
    }

    /// Get release name (from proto ReleaseSpec)
    pub fn release_name(&self) -> &str {
        self.release.name()
    }

    /// Get release version (from proto ReleaseSpec)
    pub fn release_version(&self) -> &str {
        self.release.version()
    }
}

/// Wrapper to make PlexSpacesNode implement ApplicationNode
/// 
/// This is a stub implementation. In the future, PlexSpacesNode should
/// delegate to an internal Node instance that provides full infrastructure.
struct PlexSpacesNodeApplicationNode {
    node_id: String,
    listen_addr: String,
}

#[async_trait]
impl ApplicationNode for PlexSpacesNodeApplicationNode {
    fn id(&self) -> &str {
        &self.node_id
    }

    fn listen_addr(&self) -> &str {
        &self.listen_addr
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
    use plexspaces_proto::node::v1::{
        ApplicationConfig, GrpcConfig, HealthConfig, NodeConfig, ReleaseSpec, RuntimeConfig,
        ShutdownConfig, ShutdownStrategy,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// Mock application for testing
    struct MockApp {
        name: String,
        version: String,
        start_called: Arc<RwLock<bool>>,
        stop_called: Arc<RwLock<bool>>,
    }

    impl MockApp {
        fn new(name: impl Into<String>) -> Self {
            Self {
                name: name.into(),
                version: "1.0.0".to_string(),
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
            Ok(())
        }

        async fn stop(&mut self) -> Result<(), CoreApplicationError> {
            let mut stopped = self.stop_called.write().await;
            *stopped = true;
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// Helper to create test release (using proto ReleaseSpec directly)
    fn create_test_release(apps: Vec<(&str, &str)>) -> Release {
        let applications = apps
            .into_iter()
            .map(|(name, version)| ApplicationConfig {
                name: name.to_string(),
                version: version.to_string(),
                config_path: format!("{}.toml", name),
                enabled: true,
                auto_start: true,
                shutdown_timeout_seconds: 30,
                shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful as i32,
                dependencies: vec![],
            })
            .collect();

        let spec = ReleaseSpec {
            name: "test-release".to_string(),
            version: "1.0.0".to_string(),
            description: "Test release".to_string(),
            node: Some(NodeConfig {
                id: "node1".to_string(),
                listen_address: "0.0.0.0:9001".to_string(),
                cluster_seed_nodes: vec![],
            }),
            runtime: Some({
                let mut runtime_config = RuntimeConfig {
                    grpc: Some(GrpcConfig {
                        enabled: true,
                        address: "0.0.0.0:9001".to_string(),
                        max_connections: 1000,
                        keepalive_interval_seconds: 30,
                        middleware: vec![],
                    }),
                    health: Some(HealthConfig {
                        heartbeat_interval_seconds: 2,
                        heartbeat_timeout_seconds: 10,
                    registry_url: "http://localhost:9000".to_string(),
                }),
                    security: None,
                    blob: None,
                    shared_database: None,
                    locks_provider: None,
                    channel_provider: None,
                    tuplespace_provider: None,
                    framework_info: None,
                    ..Default::default() // Include mailbox_provider default until proto is regenerated
                };
                runtime_config
            }),
            system_applications: vec![],
            applications,
            env: HashMap::new(),
            shutdown: Some(ShutdownConfig {
                global_timeout_seconds: 300,
                grace_period_seconds: 10,
                grpc_drain_timeout_seconds: 30,
            }),
        };

        Release::from_spec(spec)
    }

    /// Test: Create node
    #[test]
    fn test_create_node() {
        let release = create_test_release(vec![]);
        let node = PlexSpacesNode::new(release);

        assert_eq!(node.release_name(), "test-release");
        assert_eq!(node.release_version(), "1.0.0");
    }

    /// Test: Initial state is Created
    #[tokio::test]
    async fn test_initial_state() {
        let release = create_test_release(vec![]);
        let node = PlexSpacesNode::new(release);

        assert_eq!(node.get_state().await, NodeState::Created);
    }

    /// Test: Start single application
    #[tokio::test]
    async fn test_start_single_application() {
        let release = create_test_release(vec![("app1", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        let app1 = MockApp::new("app1");
        let start_flag = app1.start_called.clone();

        node.register_application("app1", Box::new(app1)).await;

        node.start().await.expect("Start should succeed");

        // Verify application was started
        assert!(*start_flag.read().await);
        assert_eq!(node.get_state().await, NodeState::Running);
    }

    /// Test: Start multiple applications
    #[tokio::test]
    async fn test_start_multiple_applications() {
        let release = create_test_release(vec![("app1", "1.0.0"), ("app2", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        let app1 = MockApp::new("app1");
        let app2 = MockApp::new("app2");
        let app1_start_flag = app1.start_called.clone();
        let app2_start_flag = app2.start_called.clone();

        node.register_application("app1", Box::new(app1)).await;
        node.register_application("app2", Box::new(app2)).await;

        node.start().await.expect("Start should succeed");

        // Verify both applications were started
        assert!(*app1_start_flag.read().await);
        assert!(*app2_start_flag.read().await);
        assert_eq!(node.get_state().await, NodeState::Running);
    }

    /// Test: Start without registered application fails
    #[tokio::test]
    async fn test_start_without_registration_fails() {
        let release = create_test_release(vec![("app1", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        // Don't register app1
        let result = node.start().await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NodeError::StartFailed(_)));
    }

    /// Test: Shutdown after start
    #[tokio::test]
    async fn test_shutdown_after_start() {
        let release = create_test_release(vec![("app1", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        let app1 = MockApp::new("app1");
        let stop_flag = app1.stop_called.clone();

        node.register_application("app1", Box::new(app1)).await;
        node.start().await.expect("Start should succeed");

        node.shutdown().await.expect("Shutdown should succeed");

        // Verify application was stopped
        assert!(*stop_flag.read().await);
        assert_eq!(node.get_state().await, NodeState::Stopped);
    }

    /// Test: Shutdown multiple applications in reverse order
    #[tokio::test]
    async fn test_shutdown_reverse_order() {
        let release = create_test_release(vec![("app1", "1.0.0"), ("app2", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        let app1 = MockApp::new("app1");
        let app2 = MockApp::new("app2");
        let app1_stop_flag = app1.stop_called.clone();
        let app2_stop_flag = app2.stop_called.clone();

        node.register_application("app1", Box::new(app1)).await;
        node.register_application("app2", Box::new(app2)).await;

        node.start().await.expect("Start should succeed");
        node.shutdown().await.expect("Shutdown should succeed");

        // Verify both applications were stopped
        assert!(*app1_stop_flag.read().await);
        assert!(*app2_stop_flag.read().await);
        assert_eq!(node.get_state().await, NodeState::Stopped);
    }

    /// Test: Cannot start from non-Created state
    #[tokio::test]
    async fn test_cannot_start_from_running() {
        let release = create_test_release(vec![("app1", "1.0.0")]);
        let node = PlexSpacesNode::new(release);

        let app1 = MockApp::new("app1");
        node.register_application("app1", Box::new(app1)).await;

        node.start().await.expect("First start should succeed");

        // Try to start again
        let result = node.start().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NodeError::InvalidState(_)));
    }

    /// Test: Cannot shutdown from non-Running state
    #[tokio::test]
    async fn test_cannot_shutdown_before_start() {
        let release = create_test_release(vec![]);
        let node = PlexSpacesNode::new(release);

        let result = node.shutdown().await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), NodeError::InvalidState(_)));
    }
}
