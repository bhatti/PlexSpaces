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

//! Cluster management for coordinating multiple PlexSpaces nodes.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use plexspaces_proto::node::v1::NodeCapabilities as ProtoNodeCapabilities;

use crate::{Node, NodeError, NodeId};

// Use proto-generated NodeCapabilities
type NodeCapabilities = ProtoNodeCapabilities;

/// Cluster manager for coordinating multiple nodes
pub struct ClusterManager {
    /// Local node
    local_node: Arc<Node>,
    /// Cluster configuration
    config: ClusterConfig,
    /// Cluster state
    #[allow(dead_code)]
    state: Arc<RwLock<ClusterState>>,
}

/// Cluster configuration
#[derive(Debug, Clone)]
pub struct ClusterConfig {
    /// Cluster name
    pub name: String,
    /// Seed nodes for discovery
    pub seed_nodes: Vec<(NodeId, String)>,
    /// Minimum nodes for quorum
    pub min_nodes: usize,
    /// Enable auto-discovery
    pub auto_discovery: bool,
}

/// Cluster state
#[derive(Debug)]
#[allow(dead_code)]
struct ClusterState {
    /// Current leader (if any)
    leader: Option<NodeId>,
    /// Cluster members
    members: HashMap<NodeId, NodeInfo>,
    /// Cluster epoch
    epoch: u64,
}

/// Node information in cluster
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NodeInfo {
    id: NodeId,
    address: String,
    capabilities: NodeCapabilities,
    last_seen: tokio::time::Instant,
}

impl ClusterManager {
    /// Create a new cluster manager
    pub fn new(local_node: Arc<Node>, config: ClusterConfig) -> Self {
        ClusterManager {
            local_node,
            config,
            state: Arc::new(RwLock::new(ClusterState {
                leader: None,
                members: HashMap::new(),
                epoch: 0,
            })),
        }
    }

    /// Join the cluster
    pub async fn join(&self) -> Result<(), NodeError> {
        // Connect to seed nodes via NodeRegistry
        let service_locator: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.local_node.service_locator().clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
        if let Some(node_registry) = service_locator.get_node_registry().await {
            let ctx = service_locator
                .request_context_for_system_operations()
                .await;
            for (node_id, address) in &self.config.seed_nodes {
                if node_id != self.local_node.id() {
                    // Register the seed node in NodeRegistry
                    let registration = plexspaces_proto::node::v1::NodeRegistration {
                        node_id: node_id.as_str().to_string(),
                        node_address: address.clone(),
                        ..Default::default()
                    };
                    if let Err(e) = node_registry.register_node(&ctx, registration).await {
                        tracing::warn!(node_id = %node_id.as_str(), address = %address, error = %e, "Failed to register seed node");
                    }
                }
            }
        }
        Ok(())
    }

    /// Leave the cluster
    pub async fn leave(&self) -> Result<(), NodeError> {
        Ok(())
    }
}
