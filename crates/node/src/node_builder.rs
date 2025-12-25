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
//!     .with_listen_address("0.0.0.0:9000")
//!     .build();
//! ```
//!
//! ### Node with Custom Configuration
//! ```rust,ignore
//! let node = NodeBuilder::new("production-node")
//!     .with_listen_address("0.0.0.0:9001")
//!     .with_max_connections(200)
//!     .with_heartbeat_interval_ms(10000)
//!     .with_clustering_enabled(true)
//!     .build();
//! ```

use crate::{Node, NodeId, ReleaseSpec};
use plexspaces_proto::node::v1::NodeRuntimeConfig;

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
    config: NodeRuntimeConfig,
    release_spec: Option<ReleaseSpec>,
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
        // Use default_node_config() helper which creates NodeRuntimeConfig with correct fields
        let mut config = crate::default_node_config();
        Self {
            node_id: node_id.into(),
            config,
            release_spec: None,
        }
    }

    /// Set the listen address for this node
    ///
    /// ## Arguments
    /// * `address` - Listen address (e.g., "0.0.0.0:9000")
    ///
    /// ## Example
    /// ```rust,ignore
    /// let builder = NodeBuilder::new("my-node")
    ///     .with_listen_address("0.0.0.0:9000");
    /// ```
    pub fn with_listen_address(mut self, address: impl Into<String>) -> Self {
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
    /// * `interval_ms` - Heartbeat interval in milliseconds (default: 5000)
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
    /// Sets metadata indicating that in-memory backends should be used.
    /// This is useful for testing and development.
    ///
    /// ## Note
    /// Actual backend configuration is done via ConfigBootstrap and environment variables.
    /// This method sets metadata that can be used by configuration loaders.
    ///
    /// ## Example
    /// ```rust,ignore
    /// let node = NodeBuilder::new("test-node")
    ///     .with_in_memory_backends()
    ///     .build();
    /// ```
    pub fn with_in_memory_backends(mut self) -> Self {
        self.config.metadata.insert("backend.channel".to_string(), "in-memory".to_string());
        self.config.metadata.insert("backend.tuplespace".to_string(), "in-memory".to_string());
        self.config.metadata.insert("backend.journaling".to_string(), "in-memory".to_string());
        self.config.metadata.insert("backend.keyvalue".to_string(), "in-memory".to_string());
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
        self.config.metadata.insert("backend.channel".to_string(), "redis".to_string());
        self.config.metadata.insert("backend.tuplespace".to_string(), "redis".to_string());
        self.config.metadata.insert("backend.journaling".to_string(), "sqlite".to_string());
        self.config.metadata.insert("backend.keyvalue".to_string(), "redis".to_string());
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
        self.config.metadata.insert("backend.channel".to_string(), "postgres".to_string());
        self.config.metadata.insert("backend.tuplespace".to_string(), "postgres".to_string());
        self.config.metadata.insert("backend.journaling".to_string(), "postgres".to_string());
        self.config.metadata.insert("backend.keyvalue".to_string(), "postgres".to_string());
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
        self.config.metadata.insert("backend.journaling".to_string(), "sqlite".to_string());
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

    /// Build the node with the configured options
    ///
    /// ## Returns
    /// * `Node` - The configured node instance with all services initialized
    ///
    /// ## Defaults
    /// - Listen address: "0.0.0.0:9000" if not provided
    /// - Max connections: 100 if not provided
    /// - Heartbeat interval: 5000ms if not provided
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
    ///     .with_listen_address("0.0.0.0:9000")
    ///     .build()
    ///     .await;
    /// // Node is ready to use - services are initialized
    /// ```
    pub async fn build(self) -> Node {
        let mut node = Node::new(self.node_id, self.config);
        
        // Set release_spec if provided
        if let Some(release_spec) = self.release_spec {
            node.set_release_spec(release_spec).await;
        }
        
        // Initialize all services immediately
        node.initialize_services().await
            .expect("Failed to initialize services in NodeBuilder::build()");
        
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_node_builder_with_defaults() {
        let node = NodeBuilder::new(NodeId::new("test-node")).build().await;

        assert_eq!(node.id().as_str(), "test-node");
        // Verify default config
        let config = node.config();
        assert_eq!(config.listen_addr, "0.0.0.0:9000");
        assert_eq!(config.max_connections, 100);
        assert_eq!(config.heartbeat_interval_ms, 5000);
        assert!(config.clustering_enabled);
    }

    #[tokio::test]
    async fn test_node_builder_with_listen_address() {
        let node = NodeBuilder::new("test-node")
            .with_listen_address("127.0.0.1:8080")
            .build().await;

        assert_eq!(node.config().listen_addr, "127.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_node_builder_with_max_connections() {
        let node = NodeBuilder::new("test-node")
            .with_max_connections(200)
            .build().await;

        assert_eq!(node.config().max_connections, 200);
    }

    #[tokio::test]
    async fn test_node_builder_with_heartbeat_interval() {
        let node = NodeBuilder::new("test-node")
            .with_heartbeat_interval_ms(10000)
            .build().await;

        assert_eq!(node.config().heartbeat_interval_ms, 10000);
    }

    #[tokio::test]
    async fn test_node_builder_with_clustering() {
        let node = NodeBuilder::new("test-node")
            .with_clustering_enabled(false)
            .build().await;

        assert!(!node.config().clustering_enabled);
    }

    #[tokio::test]
    async fn test_node_builder_with_metadata() {
        let node = NodeBuilder::new("test-node")
            .with_metadata("environment", "production")
            .with_metadata("region", "us-east-1")
            .build().await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("environment"), Some(&"production".to_string()));
        assert_eq!(metadata.get("region"), Some(&"us-east-1".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_fluent_api() {
        let node = NodeBuilder::new("test-node")
            .with_listen_address("0.0.0.0:9001")
            .with_max_connections(150)
            .with_heartbeat_interval_ms(7500)
            .with_clustering_enabled(true)
            .with_metadata("env", "test")
            .build().await;

        assert_eq!(node.id().as_str(), "test-node");
        assert_eq!(node.config().listen_addr, "0.0.0.0:9001");
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
            .build().await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("backend.channel"), Some(&"in-memory".to_string()));
        assert_eq!(metadata.get("backend.tuplespace"), Some(&"in-memory".to_string()));
        assert_eq!(metadata.get("backend.journaling"), Some(&"in-memory".to_string()));
        assert_eq!(metadata.get("backend.keyvalue"), Some(&"in-memory".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_with_redis_backends() {
        let node = NodeBuilder::new("test-node")
            .with_redis_backends()
            .build().await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("backend.channel"), Some(&"redis".to_string()));
        assert_eq!(metadata.get("backend.tuplespace"), Some(&"redis".to_string()));
        assert_eq!(metadata.get("backend.journaling"), Some(&"sqlite".to_string()));
        assert_eq!(metadata.get("backend.keyvalue"), Some(&"redis".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_with_postgres_backends() {
        let node = NodeBuilder::new("test-node")
            .with_postgres_backends()
            .build().await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("backend.channel"), Some(&"postgres".to_string()));
        assert_eq!(metadata.get("backend.tuplespace"), Some(&"postgres".to_string()));
        assert_eq!(metadata.get("backend.journaling"), Some(&"postgres".to_string()));
        assert_eq!(metadata.get("backend.keyvalue"), Some(&"postgres".to_string()));
    }

    #[tokio::test]
    async fn test_node_builder_with_sqlite_journaling() {
        let node = NodeBuilder::new("test-node")
            .with_sqlite_journaling()
            .build().await;

        let metadata = &node.config().metadata;
        assert_eq!(metadata.get("backend.journaling"), Some(&"sqlite".to_string()));
    }
}

