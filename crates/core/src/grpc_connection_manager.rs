// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! gRPC Connection Manager with Connection Pooling
//!
//! ## Purpose
//! Manages gRPC client connections to remote nodes with connection pooling.
//! Each service type has its own pool per node, bounded by `pool_size`.
//!
//! ## Design
//! - Channels are lazy-connected (no TCP until first RPC call).
//! - Tonic `Channel` is cheaply cloneable; all clones share the same HTTP/2 connection,
//!   so concurrent scatter-gather bursts never open extra file descriptors.
//! - The pool grows up to `pool_size` distinct channels per (ServiceType, node_id).
//!   Round-robin checkout spreads HTTP/2 stream load across the pool.
//! - File-descriptor usage is strictly bounded: at most
//!   `pool_size × num_service_types × num_nodes` TCP connections system-wide.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};

/// Service type identifier for gRPC connection pooling
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ServiceType {
    /// Application service for application lifecycle and status
    ApplicationService,
    /// Actor service for actor lifecycle and messaging
    ActorService,
    /// TupleSpace service for Linda-style tuple coordination
    TupleSpaceService,
    /// Channel service for pub/sub messaging
    ChannelService,
    /// Blob service for binary large object storage
    BlobService,
    /// Workflow service for workflow orchestration
    WorkflowService,
    /// System service for node management
    SystemService,
    /// Process group service for Erlang-style process groups (pub/sub)
    ProcessGroupService,
    /// Node service for node management and discovery
    NodeService,
}

/// Connection pool for a specific service type and node.
///
/// Each entry is a lazy-connected tonic `Channel`. Cloning a `Channel` is free and
/// multiplexes over the same HTTP/2 connection — no new TCP socket or FD is opened.
/// The pool grows to `pool_size` on demand; callers always receive a clone.
struct ServiceConnectionPool {
    channels: Vec<Channel>,
    pool_size: usize,
    node_address: String,
}

impl ServiceConnectionPool {
    fn new(node_address: String, pool_size: usize) -> Self {
        Self {
            channels: Vec::with_capacity(pool_size.max(1)),
            pool_size: pool_size.max(1),
            node_address,
        }
    }

    /// Return a clone of a pooled channel, adding a new lazy channel if pool has room.
    fn get(&mut self) -> Result<Channel, tonic::transport::Error> {
        if self.channels.len() < self.pool_size {
            let channel = Endpoint::from_shared(self.node_address.clone())?.connect_lazy();
            self.channels.push(channel);
        }
        self.channels.rotate_left(1);
        Ok(self.channels[self.channels.len() - 1].clone())
    }
}

/// gRPC Connection Manager with bounded lazy connection pooling.
pub struct GrpcConnectionManager {
    pools: Arc<RwLock<HashMap<(ServiceType, String), ServiceConnectionPool>>>,
    pool_size: usize,
}

impl GrpcConnectionManager {
    /// Create a new connection manager.
    ///
    /// `pool_size` sets the maximum number of distinct TCP connections per
    /// (ServiceType, node_id) pair. Defaults to 2. Must be ≥ 1.
    pub fn new(pool_size: Option<u32>) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            pool_size: pool_size.unwrap_or(2).max(1) as usize,
        }
    }

    /// Get a channel for the given service type and node.
    ///
    /// Returns a clone of a pooled lazy channel. The clone shares the underlying
    /// HTTP/2 connection — no new TCP socket is opened per call.
    pub async fn get_connection(
        &self,
        service_type: ServiceType,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        let key = (service_type, node_id.to_string());
        let mut pools = self.pools.write().await;
        let pool = pools
            .entry(key)
            .or_insert_with(|| ServiceConnectionPool::new(node_address.to_string(), self.pool_size));
        if pool.node_address != node_address {
            *pool = ServiceConnectionPool::new(node_address.to_string(), self.pool_size);
        }
        pool.get()
    }

    pub async fn get_actor_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ActorService, node_id, node_address).await
    }

    pub async fn get_application_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ApplicationService, node_id, node_address).await
    }

    pub async fn get_tuplespace_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::TupleSpaceService, node_id, node_address).await
    }

    pub async fn get_process_group_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ProcessGroupService, node_id, node_address).await
    }

    pub async fn get_node_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::NodeService, node_id, node_address).await
    }

    pub async fn shutdown(&self) {
        self.pools.write().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn updates_pool_address_when_node_endpoint_changes() {
        let manager = GrpcConnectionManager::new(Some(2));

        {
            let mut pools = manager.pools.write().await;
            pools.insert(
                (ServiceType::ActorService, "node-a".to_string()),
                ServiceConnectionPool::new("http://0.0.0.0:8093".to_string(), 2),
            );
        }

        let _ = manager
            .get_connection(ServiceType::ActorService, "node-a", "http://localhost:8093")
            .await;

        let pools = manager.pools.read().await;
        let pool = pools
            .get(&(ServiceType::ActorService, "node-a".to_string()))
            .expect("pool should exist");
        assert_eq!(pool.node_address, "http://localhost:8093");
    }

    #[tokio::test]
    async fn pool_grows_to_pool_size_and_rotates() {
        let manager = GrpcConnectionManager::new(Some(2));
        for _ in 0..4 {
            let _ = manager
                .get_connection(ServiceType::ActorService, "node-b", "http://localhost:8093")
                .await;
        }
        let pools = manager.pools.read().await;
        let pool = pools
            .get(&(ServiceType::ActorService, "node-b".to_string()))
            .expect("pool should exist");
        assert_eq!(pool.channels.len(), 2, "pool must not exceed pool_size");
    }
}
