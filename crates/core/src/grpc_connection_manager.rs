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

//! gRPC Connection Manager with Connection Pooling
//!
//! ## Purpose
//! Manages gRPC client connections to remote nodes with connection pooling.
//! Each service (ActorService, TupleSpaceService, etc.) has its own connection pool.
//!
//! ## Design
//! - Connection pooling per service type and node
//! - Configurable pool size (default: 2, from NodeConfig)
//! - Reuses connections for efficiency
//! - Stores default tenant-id and namespace for internal operations

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Endpoint};

/// Service type identifier
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum ServiceType {
    ActorService,
    TupleSpaceService,
    ChannelService,
    BlobService,
    WorkflowService,
    SystemService,
}

/// Connection pool for a specific service type and node
struct ServiceConnectionPool {
    /// Pool of connections (FIFO queue)
    connections: Vec<Channel>,
    /// Maximum pool size
    pool_size: usize,
    /// Service type
    service_type: ServiceType,
    /// Node address
    node_address: String,
}

impl ServiceConnectionPool {
    fn new(service_type: ServiceType, node_address: String, pool_size: usize) -> Self {
        Self {
            connections: Vec::with_capacity(pool_size),
            pool_size,
            service_type,
            node_address,
        }
    }

    /// Get a connection from the pool, or create a new one if pool is empty
    async fn get_connection(&mut self) -> Result<Channel, tonic::transport::Error> {
        // Try to reuse connection from pool
        while let Some(channel) = self.connections.pop() {
            // Check if connection is still valid by trying to get its state
            // If channel is closed, it will fail when we try to use it, so we just try to reuse it
            // The actual validity check happens when the channel is used
            return Ok(channel);
        }
        
        // Pool is empty, create new connection
        let endpoint = Endpoint::from_shared(self.node_address.clone())?;
        endpoint.connect().await
    }

    /// Return a connection to the pool (if pool is not full)
    fn return_connection(&mut self, channel: Channel) {
        if self.connections.len() < self.pool_size {
            self.connections.push(channel);
        }
    }
}

/// gRPC Connection Manager with connection pooling
pub struct GrpcConnectionManager {
    /// Connection pools: (service_type, node_id) -> ServiceConnectionPool
    pools: Arc<RwLock<HashMap<(ServiceType, String), ServiceConnectionPool>>>,
    /// Default tenant ID for internal operations
    default_tenant_id: String,
    /// Default namespace for internal operations
    default_namespace: String,
    /// Connection pool size (from NodeConfig, default: 2)
    pool_size: usize,
}

impl GrpcConnectionManager {
    /// Create a new connection manager
    ///
    /// ## Arguments
    /// * `default_tenant_id` - Default tenant ID for internal operations
    /// * `default_namespace` - Default namespace for internal operations
    /// * `pool_size` - Connection pool size per service (default: 2)
    pub fn new(
        default_tenant_id: String,
        default_namespace: String,
        pool_size: Option<u32>,
    ) -> Self {
        Self {
            pools: Arc::new(RwLock::new(HashMap::new())),
            default_tenant_id,
            default_namespace,
            pool_size: pool_size.unwrap_or(2) as usize,
        }
    }

    /// Get a connection for a specific service type and node
    ///
    /// ## Arguments
    /// * `service_type` - Type of service (ActorService, TupleSpaceService, etc.)
    /// * `node_id` - Node ID (used to lookup address via ObjectRegistry)
    /// * `node_address` - gRPC address of the node (e.g., "http://localhost:8000")
    ///
    /// ## Returns
    /// Channel ready for use, or error if connection failed
    pub async fn get_connection(
        &self,
        service_type: ServiceType,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        let key = (service_type.clone(), node_id.to_string());
        
        let mut pools = self.pools.write().await;
        let pool = pools.entry(key.clone())
            .or_insert_with(|| {
                ServiceConnectionPool::new(
                    service_type,
                    node_address.to_string(),
                    self.pool_size,
                )
            });
        
        pool.get_connection().await
    }

    /// Get a connection for ActorService (convenience method)
    ///
    /// ## Arguments
    /// * `node_id` - Node ID
    /// * `node_address` - gRPC address of the node
    ///
    /// ## Returns
    /// Channel ready for use, or error if connection failed
    pub async fn get_actor_service_connection(
        &self,
        node_id: &str,
        node_address: &str,
    ) -> Result<Channel, tonic::transport::Error> {
        self.get_connection(ServiceType::ActorService, node_id, node_address).await
    }

    /// Return a connection to the pool
    ///
    /// ## Arguments
    /// * `service_type` - Type of service
    /// * `node_id` - Node ID
    /// * `channel` - Channel to return to pool
    pub async fn return_connection(
        &self,
        service_type: ServiceType,
        node_id: &str,
        channel: Channel,
    ) {
        let key = (service_type, node_id.to_string());
        let mut pools = self.pools.write().await;
        if let Some(pool) = pools.get_mut(&key) {
            pool.return_connection(channel);
        }
    }

    /// Get default tenant ID
    pub fn default_tenant_id(&self) -> &str {
        &self.default_tenant_id
    }

    /// Get default namespace
    pub fn default_namespace(&self) -> &str {
        &self.default_namespace
    }

    /// Shutdown all connection pools
    pub async fn shutdown(&self) {
        let mut pools = self.pools.write().await;
        pools.clear();
    }
}

