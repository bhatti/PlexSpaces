// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! # Node Builder
//!
//! ## Purpose
//! Provides a fluent, builder-style API for creating nodes with sensible defaults.
//! This is part of Option C: Unified Actor Design - making the framework feel like ONE.
//!
//! ## Design Principles
//! - **Simplicity**: Sensible defaults, minimal required configuration
//! - **Intuitive**: Easy to create nodes with common configurations
//! - **Consistency**: One way to create nodes, regardless of use case
//!
//! ## Examples
//!
//! ### Simple Node Creation
//! ```rust,ignore
//! use plexspaces_node::{Node, NodeBuilder};
//!
//! let node = NodeBuilder::new("my-node")
//!     .with_listen_addr("0.0.0.0:8000")
//!     .build();
//! ```
//!
//! ### Node with Custom Configuration
//! ```rust,ignore
//! let node = NodeBuilder::new("production-node")
//!     .with_listen_addr("0.0.0.0:8000")
//!     .with_max_connections(200)
//!     .with_heartbeat_interval_ms(10000)
//!     .with_clustering_enabled(true)
//!     .build();
//! ```

use crate::{Node, NodeId, ReleaseSpec};
use plexspaces_proto::node::v1::NodeConfig;

/// Builder for creating nodes with a fluent API
///
/// ## Purpose
/// Simplifies node creation by providing sensible defaults and a fluent interface.
/// This is the unified way to create nodes in PlexSpaces.
///
/// ## Design
/// - Uses builder pattern for configuration
/// - Provides sensible defaults (listen address, connections, etc.)
/// - Single entry point for all node types
pub struct NodeBuilder {
    node_id: NodeId,
    config: NodeConfig,
    release_spec: Option<ReleaseSpec>,
    disable_auth: bool,
}

impl NodeBuilder {
    /// Create a new node builder with the given node ID
    ///
    /// ## Arguments
    /// * `node_id` - The node identifier (string or NodeId)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node");
    /// ```
    pub fn new(node_id: impl Into<NodeId>) -> Self {
        let node_id = node_id.into();
        let mut config = crate::default_node_config();
        config.id = node_id.as_str().to_string();
        Self {
            node_id,
            config,
            release_spec: None,
            disable_auth: false,
        }
    }

    /// Set the listen address for this node
    ///
    /// ## Arguments
    /// * `address` - Listen address (e.g., "0.0.0.0:8000")
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_listen_addr("0.0.0.0:8000");
    /// ```
    pub fn with_listen_addr(mut self, address: impl Into<String>) -> Self {
        self.config.listen_addr = address.into();
        self
    }

    /// Set the maximum number of connections
    ///
    /// ## Arguments
    /// * `max` - Maximum connections (default: 100)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_max_connections(200);
    /// ```
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.config.max_connections = max as u32;
        self
    }

    /// Set the heartbeat interval in milliseconds
    ///
    /// ## Arguments
    /// * `interval_ms` - Heartbeat interval in milliseconds (default: 10000)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_heartbeat_interval_ms(10000);
    /// ```
    pub fn with_heartbeat_interval_ms(mut self, interval_ms: u64) -> Self {
        self.config.heartbeat_interval_ms = interval_ms;
        self
    }

    /// Enable or disable clustering
    ///
    /// ## Arguments
    /// * `enabled` - Whether clustering is enabled (default: true)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_clustering_enabled(false);
    /// ```
    pub fn with_clustering_enabled(mut self, enabled: bool) -> Self {
        self.config.clustering_enabled = enabled;
        self
    }

    /// Set the cluster name for cluster isolation
    ///
    /// ## Purpose
    /// Sets the cluster name which is used as namespace for node registration.
    /// Nodes in the same cluster can see each other, nodes in different clusters are isolated.
    ///
    /// ## Arguments
    /// * `cluster_name` - Cluster name (used as namespace for ObjectRegistry)
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_cluster_name("production-cluster");
    /// ```
    pub fn with_cluster_name(mut self, cluster_name: impl Into<String>) -> Self {
        self.config.cluster_name = cluster_name.into();
        self
    }

    /// Add metadata to the node configuration
    ///
    /// ## Arguments
    /// * `key` - Metadata key
    /// * `value` - Metadata value
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_metadata("environment", "production");
    /// ```
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.metadata.insert(key.into(), value.into());
        self
    }

    /// Configure node to use in-memory backends (for testing)
    ///
    /// ## Purpose
    /// Configures the node to use in-memory backends for all storage services.
    /// This is the proper way to configure tests - no environment variables needed.
    ///
    /// ## How it works
    /// Sets `RuntimeConfig.db.connection_string = "sqlite::memory:"`
    /// which triggers in-memory backend selection in service initialization.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("test-node")
    ///     .with_in_memory_backends()
    ///     .build()
    ///     .await;
    /// ```
    pub fn with_in_memory_backends(mut self) -> Self {
        use plexspaces_proto::node::v1::{ReleaseSpec, RuntimeConfig};
        use plexspaces_proto::storage::v1::SharedDbConfig;

        // Create or update release_spec with in-memory database configuration
        let release_spec = self.release_spec.take().unwrap_or_else(|| ReleaseSpec {
            name: "test".to_string(),
            version: "0.0.0".to_string(),
            ..Default::default()
        });

        // Create RuntimeConfig with in-memory database
        let runtime = RuntimeConfig {
            db: Some(SharedDbConfig {
                connection_string: "sqlite::memory:".to_string(),
                pool_size: 1,
                auto_migrate: true,
                ..Default::default()
            }),
            ..release_spec.runtime.unwrap_or_default()
        };

        self.release_spec = Some(ReleaseSpec {
            runtime: Some(runtime),
            ..release_spec
        });

        // Also set metadata for components that read it directly
        self.config
            .metadata
            .insert("backend.channel".to_string(), "in-memory".to_string());
        self.config
            .metadata
            .insert("backend.tuplespace".to_string(), "in-memory".to_string());
        self.config
            .metadata
            .insert("backend.journaling".to_string(), "in-memory".to_string());
        self.config
            .metadata
            .insert("backend.keyvalue".to_string(), "in-memory".to_string());
        self
    }

    /// Configure the shared database connection string used during service initialization.
    ///
    /// ## Purpose
    /// Embedded examples that want the same unified migration/bootstrap path as the
    /// full server can set the shared DB here before `build()` initializes services.
    ///
    /// Bare file paths are treated as SQLite database files and normalized to a
    /// `sqlite://...?...` connection string automatically so embedded examples can
    /// pass `workflow.db` without reimplementing server-side config shaping.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("workflow-node")
    ///     .with_shared_db_connection_string("workflow.db")
    ///     .build_started()
    ///     .await;
    /// ```
    pub fn with_shared_db_connection_string(
        mut self,
        connection_string: impl Into<String>,
    ) -> Self {
        use plexspaces_proto::node::v1::ReleaseSpec;
        use plexspaces_proto::storage::v1::SharedDbConfig;
        let connection_string = normalize_shared_db_connection_string(connection_string.into());

        let release_spec = self.release_spec.take().unwrap_or_else(|| ReleaseSpec {
            name: "embedded".to_string(),
            version: "0.0.0".to_string(),
            ..Default::default()
        });

        let mut runtime = release_spec.runtime.clone().unwrap_or_default();
        let existing_db = runtime.db.clone().unwrap_or_default();

        runtime.db = Some(SharedDbConfig {
            connection_string,
            auto_migrate: true,
            ..existing_db
        });

        self.release_spec = Some(ReleaseSpec {
            runtime: Some(runtime),
            ..release_spec
        });
        self
    }

    /// Configure node to use Redis backends (common production setup)
    ///
    /// ## Purpose
    /// Sets metadata indicating that Redis should be used for channel and TupleSpace.
    /// SQLite will be used for journaling by default.
    ///
    /// ## Note
    /// Actual backend configuration requires:
    /// - Redis URL via `PLEXSPACES_REDIS_URL` environment variable
    /// - Or configuration via ConfigBootstrap
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("production-node")
    ///     .with_redis_backends()
    ///     .build();
    /// ```
    pub fn with_redis_backends(mut self) -> Self {
        self.config
            .metadata
            .insert("backend.channel".to_string(), "redis".to_string());
        self.config
            .metadata
            .insert("backend.tuplespace".to_string(), "redis".to_string());
        self.config
            .metadata
            .insert("backend.journaling".to_string(), "sqlite".to_string());
        self.config
            .metadata
            .insert("backend.keyvalue".to_string(), "redis".to_string());
        self
    }

    /// Configure node to use PostgreSQL backends (common production setup)
    ///
    /// ## Purpose
    /// Sets metadata indicating that PostgreSQL should be used for persistent storage.
    ///
    /// ## Note
    /// Actual backend configuration requires:
    /// - PostgreSQL URL via `PLEXSPACES_POSTGRES_URL` environment variable
    /// - Or configuration via ConfigBootstrap
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("production-node")
    ///     .with_postgres_backends()
    ///     .build();
    /// ```
    pub fn with_postgres_backends(mut self) -> Self {
        self.config
            .metadata
            .insert("backend.channel".to_string(), "postgres".to_string());
        self.config
            .metadata
            .insert("backend.tuplespace".to_string(), "postgres".to_string());
        self.config
            .metadata
            .insert("backend.journaling".to_string(), "postgres".to_string());
        self.config
            .metadata
            .insert("backend.keyvalue".to_string(), "postgres".to_string());
        self
    }

    /// Configure node to use SQLite for journaling (edge deployments)
    ///
    /// ## Purpose
    /// Sets metadata indicating that SQLite should be used for journaling.
    /// Useful for edge deployments where a separate database server isn't available.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("edge-node")
    ///     .with_sqlite_journaling()
    ///     .build();
    /// ```
    pub fn with_sqlite_journaling(mut self) -> Self {
        self.config
            .metadata
            .insert("backend.journaling".to_string(), "sqlite".to_string());
        self
    }

    /// Set the release configuration for this node
    ///
    /// ## Arguments
    /// * `release_spec` - ReleaseSpec containing node and application configuration
    ///
    /// ## Purpose
    /// Allows setting release config at node creation time, which will be used
    /// to initialize NodeConfig in ServiceLocator during node.start().
    ///
    /// ## Example
    /// ```rust,ignore
    /// let release_spec = load_release_spec_from_file("release.yaml").await?;
    /// let node = NodeBuilder::new("my-node")
    ///     .with_release_spec(release_spec)
    ///     .build();
    /// ```
    pub fn with_release_spec(mut self, release_spec: ReleaseSpec) -> Self {
        self.release_spec = Some(release_spec);
        self
    }

    /// Disable authentication for this node (useful for tests)
    ///
    /// When set, all gRPC requests are accepted without tenant/auth headers.
    pub fn with_auth_disabled(mut self) -> Self {
        self.disable_auth = true;
        self
    }

    /// Build the node with the configured options
    ///
    /// ## Returns
    /// * `Node` - The configured node instance with all services initialized
    ///
    /// ## Defaults
    /// - Listen address: "0.0.0.0:8000" if not provided
    /// - Max connections: 100 if not provided
    /// - Heartbeat interval: 10000ms if not provided
    /// - Clustering: enabled if not provided
    ///
    /// ## Services Initialization
    /// This method automatically initializes all services using `create_default_service_locator`,
    /// so the node is ready to use immediately after building. No need to call `start()` for
    /// basic operations (though `start()` is still needed for gRPC server).
    ///
    /// ## Release Config
    /// If release_spec is provided, it will be set on the node and used during initialization.
    /// Otherwise, defaults will be used.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("my-node")
    ///     .with_listen_addr("0.0.0.0:8000")
    ///     .build()
    ///     .await;
    /// // Node is ready to use - services are initialized
    /// ```
    pub async fn build(self) -> Node {
        let disable_auth = self.disable_auth;
        let node = Node::new(self.node_id, self.config);

        // Set release_spec if provided explicitly.
        // Test builds must remain independent from the caller's current working directory, so
        // unit tests skip implicit release file discovery here. Production startup still supports
        // auto-loading release config in `Node::start()`.
        if let Some(release_spec) = self.release_spec {
            node.set_release_spec(release_spec).await;
        } else if !cfg!(test) {
            if let Ok(release_spec) = node.load_release_config().await {
                node.set_release_spec(release_spec).await;
            }
        } else {
            // Test builds intentionally do not auto-load release files during `build()`.
        }

        // Initialize all services immediately
        node.initialize_services()
            .await
            .expect("Failed to initialize services in NodeBuilder::build()");

        // Register security config after services are initialized
        if disable_auth {
            let sl = node.service_locator();
            use plexspaces_proto::node::v1::SecurityConfig;
            let security = SecurityConfig {
                disable_auth: true,
                oidc: None,
                ..Default::default()
            };
            sl.register_security_config(security).await;
        }

        // Remote monitor/link RPCs need a non-empty dialable base on this node's registry
        // (`Node::start` also sets this from config). `NodeBuilder::build` skips `start`, so set
        // it here from `listen_addr` for tests and embedded nodes.
        if let Some(registry) = node.service_locator().actor_registry().await {
            registry
                .set_local_listen_addr(plexspaces_common::dialable_node_address(
                    node.config().listen_addr.trim(),
                ))
                .await;
        }

        // Register gRPC transport clients so that remote actor messaging works without
        // calling `Node::start()`.  `start()` registers WS-wrapping clients on top of
        // these; here we register the plain gRPC clients which are sufficient for tests
        // and embedded nodes that do not need WebSocket thin-client support.
        {
            use plexspaces_actor::{
                GrpcActorTransportClient, GrpcNodeTransportClient, InitializableServiceLocator,
            };

            let sl = node.service_locator();
            let sl_trait: std::sync::Arc<dyn plexspaces_actor::ServiceLocator> = sl.clone();
            let grpc_actor = std::sync::Arc::new(GrpcActorTransportClient::new(sl_trait.clone()));
            let grpc_node = std::sync::Arc::new(GrpcNodeTransportClient::new(sl_trait));
            sl.register_actor_transport_client(
                grpc_actor as std::sync::Arc<dyn plexspaces_service_traits::ActorTransportClient>,
            )
            .await;
            sl.register_node_transport_client(
                grpc_node as std::sync::Arc<dyn plexspaces_service_traits::NodeTransportClient>,
            )
            .await;
        }

        node
    }

    /// Build the node, then start the full runtime in a background task.
    ///
    /// ## Purpose
    /// Embedded examples often need the same startup path as the server:
    /// release config loading, unified migrations, service initialization,
    /// and the running node runtime. This helper provides that with one call.
    ///
    /// ## Returns
    /// `Arc<Node>` so callers can use the running node immediately while the
    /// background task owns the server/runtime loop.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("example-node")
    ///     .with_clustering_enabled(false)
    ///     .build_started()
    ///     .await;
    /// ```
    pub async fn build_started(self) -> std::sync::Arc<Node> {
        let node = std::sync::Arc::new(self.build().await);
        let node_for_start = node.clone();
        tokio::spawn(async move {
            if let Err(error) = node_for_start.start().await {
                tracing::error!(error = %error, "Embedded node runtime exited with error");
            }
        });
        node
    }
}

fn normalize_shared_db_connection_string(connection_string: String) -> String {
    let trimmed = connection_string.trim();
    if trimmed.starts_with("sqlite://")
        || trimmed.starts_with("sqlite::memory:")
        || trimmed.starts_with("postgres://")
        || trimmed.starts_with("postgresql://")
        || trimmed.is_empty()
        || trimmed.contains("://")
    {
        return trimmed.to_string();
    }

    format!("sqlite://{}?mode=rwc", trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_builder_with_defaults() {
        let node = NodeBuilder::new(NodeId::new("test-node")).build().await;

        assert_eq!(node.id().as_str(), "test-node");
        // Verify default config.  heartbeat_interval_ms is 0 until config_manager::initialize()
        // resolves it; that resolution only happens during Node::start() (not build()).
        let config = node.config();
        assert_eq!(config.listen_addr, "0.0.0.0:8000");
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.heartbeat_interval_ms, 0);
        assert!(config.clustering_enabled);
    }

    #[tokio::test]
    async fn test_node_builder_with_listen_addr() {
        let node = NodeBuilder::new("test-node")
            .with_listen_addr("127.0.0.1:8080")
            .build()
            .await;

        assert_eq!(node.config().listen_addr, "127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_node_builder_with_max_connections() {
        let node = NodeBuilder::new("test-node")
            .with_max_connections(200)
            .build()
            .await;

        assert_eq!(node.config().max_connections, 200);
    }

    #[tokio::test]
    async fn test_node_builder_with_heartbeat_interval() {
        let node = NodeBuilder::new("test-node")
            .with_heartbeat_interval_ms(10000)
            .build()
            .await;

        assert_eq!(node.config().heartbeat_interval_ms, 10000);
    }

    #[tokio::test]
    async fn test_node_builder_with_clustering() {
        let node = NodeBuilder::new("test-node")
            .with_clustering_enabled(false)
            .build()
            .await;

        assert!(!node.config().clustering_enabled);
    }

    #[tokio::test]
    async fn test_node_builder_with_metadata() {
        let node = NodeBuilder::new("test-node")
            .with_metadata("environment", "production")
            .with_metadata("region", "us-east-1")
            .build()
            .await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("environment"), Some(&"production".to_string()));
        assert_eq!(metadata.get("region"), Some(&"us-east-1".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_fluent_api() {
        let node = NodeBuilder::new("test-node")
            .with_listen_addr("0.0.0.0:8000")
            .with_max_connections(150)
            .with_heartbeat_interval_ms(7500)
            .with_clustering_enabled(true)
            .with_metadata("env", "test")
            .build()
            .await;

        assert_eq!(node.id().as_str(), "test-node");
        assert_eq!(node.config().listen_addr, "0.0.0.0:8000");
        assert_eq!(node.config().max_connections, 150);
        assert_eq!(node.config().heartbeat_interval_ms, 7500);
        assert!(node.config().clustering_enabled);
        assert_eq!(node.config().metadata.get("env"), Some(&"test".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_with_node_id() {
        let node_id = NodeId::new("custom-node-id");
        let node = NodeBuilder::new(node_id).build().await;

        assert_eq!(node.id().as_str(), "custom-node-id");
    }

    #[tokio::test]
    async fn test_node_builder_with_in_memory_backends() {
        let node = NodeBuilder::new("test-node")
            .with_in_memory_backends()
            .build()
            .await;

        let metadata = &node.config().metadata;
        assert_eq!(
            metadata.get("backend.channel"),
            Some(&"in-memory".to_string())
        );
        assert_eq!(
            metadata.get("backend.tuplespace"),
            Some(&"in-memory".to_string())
        );
        assert_eq!(
            metadata.get("backend.journaling"),
            Some(&"in-memory".to_string())
        );
        assert_eq!(
            metadata.get("backend.keyvalue"),
            Some(&"in-memory".to_string())
        );
    }

    #[test]
    fn test_normalize_shared_db_connection_string_file_path() {
        assert_eq!(
            normalize_shared_db_connection_string("workflow.db".to_string()),
            "sqlite://workflow.db?mode=rwc"
        );
    }

    #[test]
    fn test_normalize_shared_db_connection_string_absolute_path() {
        assert_eq!(
            normalize_shared_db_connection_string("/tmp/workflow.db".to_string()),
            "sqlite:///tmp/workflow.db?mode=rwc"
        );
    }

    #[test]
    fn test_normalize_shared_db_connection_string_preserves_explicit_urls() {
        assert_eq!(
            normalize_shared_db_connection_string("postgres://localhost/test".to_string()),
            "postgres://localhost/test"
        );
        assert_eq!(
            normalize_shared_db_connection_string("sqlite://workflow.db?mode=rwc".to_string()),
            "sqlite://workflow.db?mode=rwc"
        );
    }

    #[tokio::test]
    async fn test_node_builder_with_redis_backends() {
        let node = NodeBuilder::new("test-node")
            .with_redis_backends()
            .build()
            .await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("backend.channel"), Some(&"redis".to_string()));
        assert_eq!(
            metadata.get("backend.tuplespace"),
            Some(&"redis".to_string())
        );
        assert_eq!(
            metadata.get("backend.journaling"),
            Some(&"sqlite".to_string())
        );
        assert_eq!(metadata.get("backend.keyvalue"), Some(&"redis".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_with_postgres_backends() {
        let node = NodeBuilder::new("test-node")
            .with_postgres_backends()
            .build()
            .await;

        let metadata = &node.config().metadata;
        assert_eq!(
            metadata.get("backend.channel"),
            Some(&"postgres".to_string())
        );
        assert_eq!(
            metadata.get("backend.tuplespace"),
            Some(&"postgres".to_string())
        );
        assert_eq!(
            metadata.get("backend.journaling"),
            Some(&"postgres".to_string())
        );
        assert_eq!(
            metadata.get("backend.keyvalue"),
            Some(&"postgres".to_string())
        );
    }

    #[tokio::test]
    async fn test_node_builder_with_sqlite_journaling() {
        let node = NodeBuilder::new("test-node")
            .with_sqlite_journaling()
            .build()
            .await;

        let metadata = &node.config().metadata;
        assert_eq!(
            metadata.get("backend.journaling"),
            Some(&"sqlite".to_string())
        );
    }
}
