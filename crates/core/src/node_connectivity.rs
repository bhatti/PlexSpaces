// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// NodeConnectivity trait for connecting to remote nodes (seed nodes).
// Used by node startup (cluster_seed_nodes) and application deploy (ApplicationSpec.seed_nodes).

use async_trait::async_trait;
use std::collections::HashMap;

/// Result of connecting to a set of node addresses.
/// - `connected`: node_id -> address for successful connections
/// - `failed`: address -> error message for failures
#[derive(Debug, Default, Clone)]
pub struct ConnectNodesResult {
    /// Successfully connected nodes (node_id -> address)
    pub connected: HashMap<String, String>,
    /// Failed connections (address -> error)
    pub failed: HashMap<String, String>,
}

/// Trait for connecting to remote nodes without using gRPC directly.
/// Implemented by NodeServiceImpl; used by node startup and application deploy.
#[async_trait]
pub trait NodeConnectivity: Send + Sync {
    /// Connect to the given node addresses (gRPC format e.g. "localhost:8091" or "http://host:8091").
    /// Idempotent: already-connected nodes are skipped. Returns connected and failed maps.
    async fn connect_to_nodes(
        &self,
        node_addresses: Vec<String>,
        timeout_secs: Option<u64>,
    ) -> Result<ConnectNodesResult, String>;

    /// Connect to a list of node addresses using default timeout (5s).
    /// Assumes cluster name is the same as the local node (same-cluster check uses node config).
    /// Use for node startup (cluster_seed_nodes) and application deploy (seed_nodes).
    async fn connect_to_node_addresses(
        &self,
        node_addresses: Vec<String>,
    ) -> Result<ConnectNodesResult, String> {
        self.connect_to_nodes(node_addresses, Some(5)).await
    }
}
